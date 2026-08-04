//! Connection discovery: work out where the database is and what engine it
//! speaks, without connecting to it.
//!
//! [`resolve`] assembles the layers `SPEC.md` §6 orders and hands them to
//! [`ConnectionConfig::resolve`]. Two of those layers are built here:
//!
//! - Docker, from `docker compose config` and `docker inspect`. The daemon
//!   already resolves override files, profiles and `${VAR}` interpolation, so
//!   dbctx asks it rather than reinterpreting compose files itself.
//! - The prompt, consulted last and only when a terminal is attached.
//!
//! Nothing here opens a database connection. The engine is read from the
//! image of a discovered container, never guessed from a port or a handshake.

use std::collections::BTreeMap;
use std::process::Command;

use serde_json::Value;
use thiserror::Error;

use crate::config::{ConfigError, ConnectionConfig, ConnectionSource, Driver};

/// The layers and switches [`resolve`] needs.
///
/// The four non-discovered layers are supplied by the caller, which owns
/// reading them; discovery contributes the Docker layer and the prompt, and
/// owns the order they are all consulted in.
#[derive(Debug, Default)]
pub struct Options {
    /// Settings named on the command line.
    pub cli: ConnectionSource,
    /// Settings from `.dbctx.toml`.
    pub project: ConnectionSource,
    /// Settings from `.env`.
    pub dotenv: ConnectionSource,
    /// Settings from the process environment.
    pub environment: ConnectionSource,
    /// Compose service to discover.
    pub compose_service: Option<String>,
    /// Container to discover.
    pub docker_container: Option<String>,
    /// Whether a terminal is attached, so the prompt may be used.
    pub interactive: bool,
}

/// Resolve a connection from every source `SPEC.md` §6 lists.
///
/// The prompt runs after the other sources rather than alongside them,
/// because what it asks for depends on what they left unanswered.
pub fn resolve(options: &Options) -> Result<ConnectionConfig, DiscoveryError> {
    let docker = discovered(options)?;

    let mut sources = vec![
        options.cli.clone(),
        docker,
        options.project.clone(),
        options.dotenv.clone(),
        options.environment.clone(),
    ];

    let missing = ConnectionConfig::missing(&sources);
    if !missing.is_empty() && options.interactive {
        tracing::debug!(?missing, "asking for the settings nothing supplied");
        let stdin = std::io::stdin();
        sources.push(ConnectionSource::from_prompt(
            &missing,
            stdin.lock(),
            std::io::stderr(),
        )?);
    }

    Ok(ConnectionConfig::resolve(&sources)?)
}

/// The layer described by Docker, or nothing when none was asked for.
fn discovered(options: &Options) -> Result<ConnectionSource, DiscoveryError> {
    match (&options.compose_service, &options.docker_container) {
        (Some(service), None) => compose_service(service),
        (None, Some(container)) => container_settings(container),
        (Some(_), Some(_)) => Err(DiscoveryError::ConflictingSelectors),
        (None, None) => Ok(ConnectionSource::default()),
    }
}

/// The settings of a Compose service, via the container actually running it.
///
/// `docker compose config` would report what the compose file declares, which
/// is not the same thing: a stopped service still has a declared port, and a
/// `ports` entry naming only the container side gets a host port assigned at
/// runtime that the file cannot know. Asking which container is running the
/// service and reading that container reports what is really listening.
fn compose_service(service: &str) -> Result<ConnectionSource, DiscoveryError> {
    let json = docker(&["compose", "ps", "--all", "--format", "json"]).map_err(|error| {
        // A directory with no compose file is a mistake about where dbctx was
        // run, not about docker, so it deserves its own answer.
        if refusal_mentions(&error, "no configuration file") {
            DiscoveryError::NoComposeProject
        } else {
            error
        }
    })?;

    let container = container_for_service(&compose_containers(&json)?, service)?;
    container_settings(&container)
}

/// One container in `docker compose ps` output.
#[derive(Debug, PartialEq, Eq)]
struct ComposeContainer {
    /// The compose service it runs.
    service: String,
    /// The container name, which is what `docker inspect` takes.
    name: String,
    /// Whether it is up.
    state: String,
}

/// The containers `docker compose ps --format json` reports.
///
/// Recent versions write one object per line; older ones wrote a single
/// array. Both are accepted so the answer does not depend on which docker is
/// installed.
fn compose_containers(json: &str) -> Result<Vec<ComposeContainer>, DiscoveryError> {
    let described = if json.trim_start().starts_with('[') {
        parse_json(json, "docker compose ps")?
            .as_array()
            .cloned()
            .unwrap_or_default()
    } else {
        json.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| parse_json(line, "docker compose ps"))
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(described
        .iter()
        .filter_map(|container| {
            Some(ComposeContainer {
                service: container.get("Service")?.as_str()?.to_string(),
                name: container.get("Name")?.as_str()?.to_string(),
                state: container
                    .get("State")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect())
}

/// The name of the running container for `service`.
fn container_for_service(
    containers: &[ComposeContainer],
    service: &str,
) -> Result<String, DiscoveryError> {
    let found = containers
        .iter()
        .find(|container| container.service == service)
        .ok_or_else(|| DiscoveryError::ServiceNotFound {
            service: service.to_string(),
            known: containers
                .iter()
                .map(|container| container.service.clone())
                .collect(),
        })?;

    if found.state != "running" {
        return Err(DiscoveryError::ServiceNotRunning {
            service: service.to_string(),
            state: found.state.clone(),
        });
    }

    Ok(found.name.clone())
}

/// The settings of a running container, via `docker inspect`.
fn container_settings(container: &str) -> Result<ConnectionSource, DiscoveryError> {
    let json = docker(&["inspect", container]).map_err(|error| {
        // `docker inspect` exits non-zero for a name it does not know rather
        // than reporting nothing, so the refusal has to be read to tell a
        // misspelled container from a broken daemon.
        if refusal_mentions(&error, "no such object") {
            DiscoveryError::ContainerNotFound {
                container: container.to_string(),
            }
        } else {
            error
        }
    })?;
    let inspected = parse_json(&json, "docker inspect")?;
    source_from_inspect(&inspected, container)
}

/// Whether docker refused with a message containing `needle`.
fn refusal_mentions(error: &DiscoveryError, needle: &str) -> bool {
    matches!(
        error,
        DiscoveryError::DockerRefused { message, .. }
            if message.to_ascii_lowercase().contains(needle)
    )
}

/// Run `docker` and return its standard output.
fn docker(args: &[&str]) -> Result<String, DiscoveryError> {
    tracing::debug!(?args, "running docker");

    let output = Command::new("docker")
        .args(args)
        .output()
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                DiscoveryError::DockerMissing
            } else {
                DiscoveryError::DockerFailed {
                    command: args.join(" "),
                    source,
                }
            }
        })?;

    if !output.status.success() {
        return Err(DiscoveryError::DockerRefused {
            command: args.join(" "),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parse output that should be JSON.
fn parse_json(text: &str, command: &str) -> Result<Value, DiscoveryError> {
    serde_json::from_str(text).map_err(|source| DiscoveryError::Unreadable {
        command: command.to_string(),
        source,
    })
}

/// The connection settings in `docker inspect` output.
fn source_from_inspect(
    inspected: &Value,
    container: &str,
) -> Result<ConnectionSource, DiscoveryError> {
    let described = inspected
        .as_array()
        .and_then(|containers| containers.first())
        .ok_or_else(|| DiscoveryError::ContainerNotFound {
            container: container.to_string(),
        })?;

    let image = described
        .pointer("/Config/Image")
        .and_then(Value::as_str)
        .unwrap_or("");
    let driver = driver_from_image(image).ok_or_else(|| DiscoveryError::UnrecognisedImage {
        subject: container.to_string(),
        image: image.to_string(),
    })?;

    let environment = environment(described.pointer("/Config/Env"));
    let port = published_host_port(described.pointer("/NetworkSettings/Ports"), driver)
        .ok_or_else(|| DiscoveryError::NoPublishedPort {
            subject: container.to_string(),
            port: driver.default_port(),
        })?;

    Ok(settings(driver, port, &environment))
}

/// Assemble the settings a discovered container implies.
///
/// The host is the loopback address because the port dbctx found is one the
/// daemon published onto this machine.
fn settings(driver: Driver, port: u16, environment: &BTreeMap<String, String>) -> ConnectionSource {
    let (database, user, password) = credentials(driver, environment);

    let source = ConnectionSource {
        driver: Some(driver),
        host: Some("127.0.0.1".to_string()),
        port: Some(port),
        database,
        user,
        password,
        socket: None,
    };

    tracing::debug!(
        driver = %driver,
        port,
        database = ?source.database,
        user = ?source.user,
        "discovered connection"
    );
    source
}

/// The engine an image name names.
///
/// Checked most specific first: a SQL Server image mentions `mssql`, and a
/// MariaDB image never mentions `mysql`, but the reverse is not guaranteed.
fn driver_from_image(image: &str) -> Option<Driver> {
    let image = image.to_ascii_lowercase();
    if image.contains("mssql") || image.contains("sqlserver") || image.contains("sql-server") {
        Some(Driver::Sqlsrv)
    } else if image.contains("mariadb") {
        Some(Driver::Mariadb)
    } else if image.contains("mysql") || image.contains("percona") {
        Some(Driver::Mysql)
    } else {
        None
    }
}

/// The environment of a described container.
///
/// Compose reports a map, `docker inspect` a list of `KEY=value` strings, and
/// a compose file written as a list survives into some versions of the
/// output. All three are accepted.
fn environment(described: Option<&Value>) -> BTreeMap<String, String> {
    let mut environment = BTreeMap::new();

    match described {
        Some(Value::Object(entries)) => {
            for (key, value) in entries {
                if let Some(value) = value.as_str() {
                    environment.insert(key.clone(), value.to_string());
                }
            }
        }
        Some(Value::Array(entries)) => {
            for entry in entries.iter().filter_map(Value::as_str) {
                if let Some((key, value)) = entry.split_once('=') {
                    environment.insert(key.to_string(), value.to_string());
                }
            }
        }
        _ => {}
    }

    environment
}

/// The credentials a container's environment implies.
///
/// The official images take the database and an optional unprivileged user
/// from the environment. Where no user was configured, the superuser the
/// image always creates is the only one that exists.
fn credentials(
    driver: Driver,
    environment: &BTreeMap<String, String>,
) -> (Option<String>, Option<String>, Option<String>) {
    let value = |keys: &[&str]| keys.iter().find_map(|key| environment.get(*key)).cloned();

    match driver {
        Driver::Mysql | Driver::Mariadb => {
            let database = value(&["MYSQL_DATABASE", "MARIADB_DATABASE"]);
            match value(&["MYSQL_USER", "MARIADB_USER"]) {
                Some(user) => (
                    database,
                    Some(user),
                    value(&["MYSQL_PASSWORD", "MARIADB_PASSWORD"]),
                ),
                None => (
                    database,
                    Some("root".to_string()),
                    value(&["MYSQL_ROOT_PASSWORD", "MARIADB_ROOT_PASSWORD"]),
                ),
            }
        }
        Driver::Sqlsrv => (
            None,
            Some("sa".to_string()),
            value(&["MSSQL_SA_PASSWORD", "SA_PASSWORD"]),
        ),
    }
}

/// The host port a running container publishes for `driver`.
fn published_host_port(ports: Option<&Value>, driver: Driver) -> Option<u16> {
    let wanted = format!("{}/tcp", driver.default_port());

    ports?
        .get(&wanted)?
        .as_array()?
        .iter()
        .find_map(|binding| binding.get("HostPort")?.as_str()?.parse().ok())
}

/// Why a connection could not be discovered.
#[derive(Debug, Error)]
pub enum DiscoveryError {
    /// Configuration could not be resolved from what was discovered.
    #[error(transparent)]
    Config(#[from] ConfigError),

    /// Both selectors were given.
    #[error(
        "--compose-service and --docker-container cannot both be given\n\
         each names a different way to find the same connection\n\
         try: whichever one matches how the database is running"
    )]
    ConflictingSelectors,

    /// The docker command is not installed.
    #[error(
        "docker is not installed, or is not on PATH\n\
         dbctx asks docker where a discovered database is listening\n\
         try: naming the connection directly with --host and --port"
    )]
    DockerMissing,

    /// The docker command could not be run.
    #[error(
        "could not run `docker {command}`: {source}\n\
         dbctx asks docker where a discovered database is listening\n\
         try: naming the connection directly with --host and --port"
    )]
    DockerFailed {
        /// The arguments docker was given.
        command: String,
        /// What went wrong.
        source: std::io::Error,
    },

    /// The docker command reported a failure.
    #[error(
        "`docker {command}` failed: {message}\n\
         dbctx asks docker where a discovered database is listening\n\
         check that the daemon is running and the project is the right one"
    )]
    DockerRefused {
        /// The arguments docker was given.
        command: String,
        /// What docker said.
        message: String,
    },

    /// The docker command produced something other than JSON.
    #[error(
        "could not read the output of `{command}`: {source}\n\
         dbctx expects JSON from this command\n\
         check that the installed docker supports it"
    )]
    Unreadable {
        /// The command that was run.
        command: String,
        /// What went wrong.
        source: serde_json::Error,
    },

    /// The compose project has no container for that service.
    #[error(
        "no container for compose service `{service}`\n\
         --compose-service names a service the compose project here has \
         started at least once\n\
         known services: {}",
        if known.is_empty() { "none; try docker compose up -d".to_string() } else { known.join(", ") }
    )]
    ServiceNotFound {
        /// The name that was asked for.
        service: String,
        /// The services that have containers.
        known: Vec<String>,
    },

    /// The compose service is not up.
    #[error(
        "compose service `{service}` is not running: {state}\n\
         dbctx reads the port from the container actually listening, so the \
         service has to be up\n\
         try: docker compose up -d {service}"
    )]
    ServiceNotRunning {
        /// The service that was asked for.
        service: String,
        /// What state its container is in.
        state: String,
    },

    /// There is no compose project here.
    #[error(
        "no compose project in this directory\n\
         --compose-service reads the compose project where dbctx was run\n\
         try: running from the directory holding compose.yaml"
    )]
    NoComposeProject,

    /// No such container is running.
    #[error(
        "no container named `{container}`\n\
         --docker-container names a container docker knows about\n\
         try: docker ps to see what is running"
    )]
    ContainerNotFound {
        /// The name that was asked for.
        container: String,
    },

    /// The image is not one of the supported engines.
    #[error(
        "`{subject}` runs `{image}`, which is not a database dbctx supports\n\
         dbctx reads the engine from the image name\n\
         try: --driver mysql|mariadb|sqlsrv to say which it speaks"
    )]
    UnrecognisedImage {
        /// The service or container that was inspected.
        subject: String,
        /// The image it runs.
        image: String,
    },

    /// The container does not publish its database port.
    #[error(
        "`{subject}` does not publish port {port} to this machine\n\
         dbctx connects over a published port, not from inside the network\n\
         try: adding a ports entry, or --host and --port for how you reach it"
    )]
    NoPublishedPort {
        /// The service or container that was inspected.
        subject: String,
        /// The port that was looked for.
        port: u16,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(text: &str) -> Value {
        serde_json::from_str(text).expect("fixture is JSON")
    }

    /// `docker compose ps --format json`, which recent versions write as one
    /// object per line.
    const COMPOSE_PS: &str = concat!(
        r#"{"Name":"shop-db-1","Service":"db","State":"running","Image":"mariadb:12.2.2","#,
        r#""Publishers":[{"URL":"0.0.0.0","TargetPort":3306,"PublishedPort":3308,"Protocol":"tcp"}]}"#,
        "\n",
        r#"{"Name":"shop-cache-1","Service":"cache","State":"running","Image":"redis:7"}"#,
        "\n",
        r#"{"Name":"shop-worker-1","Service":"worker","State":"exited (0)","Image":"busybox"}"#,
        "\n"
    );

    const INSPECT: &str = r#"[{
        "Config": {
            "Image": "mariadb:11",
            "Env": ["MARIADB_DATABASE=shop", "MARIADB_ROOT_PASSWORD=root-secret", "PATH=/usr/bin"]
        },
        "NetworkSettings": {
            "Ports": {
                "3306/tcp": [{"HostIp": "0.0.0.0", "HostPort": "3308"}]
            }
        }
    }]"#;

    #[test]
    fn compose_output_is_read_as_lines_or_as_an_array() {
        let lines = compose_containers(COMPOSE_PS).unwrap();
        let array =
            compose_containers(&format!("[{}]", COMPOSE_PS.trim_end().replace('\n', ","))).unwrap();

        assert_eq!(lines, array);
        assert_eq!(
            lines[0],
            ComposeContainer {
                service: "db".to_string(),
                name: "shop-db-1".to_string(),
                state: "running".to_string(),
            }
        );
    }

    #[test]
    fn a_service_resolves_to_the_container_running_it() {
        let containers = compose_containers(COMPOSE_PS).unwrap();

        assert_eq!(
            container_for_service(&containers, "db").unwrap(),
            "shop-db-1"
        );
    }

    #[test]
    fn a_service_that_is_not_up_says_so_rather_than_reporting_a_dead_port() {
        let containers = compose_containers(COMPOSE_PS).unwrap();

        let error = container_for_service(&containers, "worker").unwrap_err();

        assert!(matches!(error, DiscoveryError::ServiceNotRunning { .. }));
        let message = error.to_string();
        assert!(message.contains("exited (0)"), "{message}");
        assert!(message.contains("docker compose up -d worker"), "{message}");
    }

    #[test]
    fn a_project_with_nothing_started_says_to_start_it() {
        let error = container_for_service(&[], "db").unwrap_err();

        assert!(
            error.to_string().contains("docker compose up -d"),
            "{error}"
        );
    }

    #[test]
    fn a_running_container_supplies_the_whole_connection() {
        let source = source_from_inspect(&json(INSPECT), "shop-db-1").unwrap();

        assert_eq!(
            source,
            ConnectionSource {
                driver: Some(Driver::Mariadb),
                host: Some("127.0.0.1".to_string()),
                port: Some(3308),
                database: Some("shop".to_string()),
                user: Some("root".to_string()),
                password: Some("root-secret".to_string()),
                socket: None,
            }
        );
    }

    #[test]
    fn an_unknown_service_lists_the_ones_that_have_containers() {
        let containers = compose_containers(COMPOSE_PS).unwrap();

        let error = container_for_service(&containers, "database").unwrap_err();

        let message = error.to_string();
        assert!(message.contains("database"), "{message}");
        assert!(message.contains("db, cache, worker"), "{message}");
    }

    #[test]
    fn a_container_running_something_else_is_reported_as_such() {
        let redis = json(r#"[{"Config": {"Image": "redis:7"}}]"#);

        let error = source_from_inspect(&redis, "shop-cache-1").unwrap_err();

        assert!(matches!(error, DiscoveryError::UnrecognisedImage { .. }));
        assert!(error.to_string().contains("redis:7"), "{error}");
    }

    #[test]
    fn a_container_that_publishes_nothing_says_which_port_was_wanted() {
        let unpublished = json(r#"[{"Config": {"Image": "mysql:8.4"}}]"#);

        let error = source_from_inspect(&unpublished, "shop-db-1").unwrap_err();

        assert!(matches!(error, DiscoveryError::NoPublishedPort { .. }));
        assert!(error.to_string().contains("3306"), "{error}");
    }

    #[test]
    fn images_name_the_engine_they_run() {
        for (image, driver) in [
            ("mysql:8.4", Some(Driver::Mysql)),
            ("mysql:8.0-debian", Some(Driver::Mysql)),
            ("percona:8.0", Some(Driver::Mysql)),
            ("mariadb:11", Some(Driver::Mariadb)),
            ("mariadb:10.11-jammy", Some(Driver::Mariadb)),
            (
                "mcr.microsoft.com/mssql/server:2022-latest",
                Some(Driver::Sqlsrv),
            ),
            (
                "mcr.microsoft.com/mssql/server:2019-latest",
                Some(Driver::Sqlsrv),
            ),
            ("redis:7", None),
            ("postgres:16", None),
            ("", None),
        ] {
            assert_eq!(driver_from_image(image), driver, "{image}");
        }
    }

    #[test]
    fn a_mariadb_image_is_never_mistaken_for_mysql() {
        assert_eq!(driver_from_image("mariadb:11"), Some(Driver::Mariadb));
        assert_eq!(driver_from_image("MariaDB:11"), Some(Driver::Mariadb));
    }

    #[test]
    fn only_the_binding_for_the_engines_own_port_is_taken() {
        let ports = json(
            r#"{
                "8080/tcp": [{"HostIp": "0.0.0.0", "HostPort": "9090"}],
                "3306/tcp": [{"HostIp": "0.0.0.0", "HostPort": "3307"}]
            }"#,
        );

        assert_eq!(published_host_port(Some(&ports), Driver::Mysql), Some(3307));
        assert_eq!(published_host_port(Some(&ports), Driver::Sqlsrv), None);
    }

    #[test]
    fn a_port_that_is_exposed_but_not_published_is_not_a_port() {
        let ports = json(r#"{"3306/tcp": null}"#);

        assert_eq!(published_host_port(Some(&ports), Driver::Mysql), None);
    }

    #[test]
    fn an_environment_is_read_as_a_map_or_as_a_list() {
        let map = json(r#"{"MYSQL_DATABASE": "shop"}"#);
        let list = json(r#"["MYSQL_DATABASE=shop"]"#);

        assert_eq!(environment(Some(&map)), environment(Some(&list)));
        assert_eq!(
            environment(Some(&map)).get("MYSQL_DATABASE").unwrap(),
            "shop"
        );
    }

    #[test]
    fn an_environment_value_containing_an_equals_sign_survives() {
        let list = json(r#"["MYSQL_ROOT_PASSWORD=a=b=c"]"#);

        assert_eq!(
            environment(Some(&list)).get("MYSQL_ROOT_PASSWORD").unwrap(),
            "a=b=c"
        );
    }

    #[test]
    fn sql_server_connects_as_sa_with_either_password_variable() {
        for key in ["MSSQL_SA_PASSWORD", "SA_PASSWORD"] {
            let environment = BTreeMap::from([(key.to_string(), "secret".to_string())]);

            let (database, user, password) = credentials(Driver::Sqlsrv, &environment);

            assert_eq!(database, None);
            assert_eq!(user.as_deref(), Some("sa"));
            assert_eq!(password.as_deref(), Some("secret"));
        }
    }

    #[test]
    fn an_unprivileged_user_is_preferred_over_the_superuser() {
        let environment = BTreeMap::from([
            ("MYSQL_USER".to_string(), "reader".to_string()),
            ("MYSQL_PASSWORD".to_string(), "secret".to_string()),
            ("MYSQL_ROOT_PASSWORD".to_string(), "root-secret".to_string()),
        ]);

        let (_, user, password) = credentials(Driver::Mysql, &environment);

        assert_eq!(user.as_deref(), Some("reader"));
        assert_eq!(password.as_deref(), Some("secret"));
    }

    #[test]
    fn a_refusal_is_read_to_tell_a_missing_object_from_a_broken_daemon() {
        let missing = DiscoveryError::DockerRefused {
            command: "inspect nonesuch".to_string(),
            message: "Error: No such object: nonesuch".to_string(),
        };
        let broken = DiscoveryError::DockerRefused {
            command: "inspect db".to_string(),
            message: "Cannot connect to the Docker daemon".to_string(),
        };

        assert!(refusal_mentions(&missing, "no such object"));
        assert!(!refusal_mentions(&broken, "no such object"));
        assert!(!refusal_mentions(
            &DiscoveryError::DockerMissing,
            "no such object"
        ));
    }

    #[test]
    fn asking_for_both_selectors_is_refused_before_docker_runs() {
        let options = Options {
            compose_service: Some("db".to_string()),
            docker_container: Some("shop-db-1".to_string()),
            ..Options::default()
        };

        let error = resolve(&options).unwrap_err();

        assert!(matches!(error, DiscoveryError::ConflictingSelectors));
    }

    #[test]
    fn resolving_without_docker_uses_the_layers_it_was_given() {
        let options = Options {
            cli: ConnectionSource {
                database: Some("shop".to_string()),
                ..ConnectionSource::default()
            },
            environment: ConnectionSource {
                driver: Some(Driver::Mariadb),
                ..ConnectionSource::default()
            },
            ..Options::default()
        };

        let config = resolve(&options).unwrap();

        assert_eq!(config.database(), "shop");
        assert_eq!(config.driver(), Driver::Mariadb);
        assert_eq!(config.port(), 3306);
    }

    #[test]
    fn a_missing_engine_is_reported_rather_than_guessed() {
        let options = Options {
            cli: ConnectionSource {
                database: Some("shop".to_string()),
                host: Some("db.internal".to_string()),
                ..ConnectionSource::default()
            },
            ..Options::default()
        };

        let error = resolve(&options).unwrap_err();

        assert!(matches!(
            error,
            DiscoveryError::Config(ConfigError::UnknownEngine)
        ));
    }
}
