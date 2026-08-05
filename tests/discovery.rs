//! Discovery against a real Docker daemon.
//!
//! Everything dbctx does with docker's output is unit tested against
//! fixtures; this covers the part fixtures cannot, which is that dbctx
//! invokes docker correctly and copes with what a real one answers.
//!
//! These tests need docker and a MySQL or MariaDB image already pulled. They
//! skip when either is absent rather than downloading several hundred
//! megabytes, so a machine without them reports green without having proved
//! anything. The Docker matrix in `TESTING.md` pins images from Phase 4, at
//! which point CI always runs them.
//!
//! No database is started: discovery never connects, so the container runs
//! `sleep` instead of the engine and starts instantly.

use std::process::{Command, Output};

/// A container that is removed however the test ends.
struct Container {
    name: String,
}

impl Drop for Container {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "--force", &self.name])
            .output();
    }
}

fn docker(args: &[&str]) -> Option<Output> {
    Command::new("docker").args(args).output().ok()
}

/// Whether a docker daemon is reachable.
fn docker_available() -> bool {
    docker(&["info", "--format", "{{.ServerVersion}}"])
        .is_some_and(|output| output.status.success())
}

/// A MySQL or MariaDB image already on this machine, if there is one.
///
/// Sorted so repeated runs pick the same image and a failure is reproducible.
fn local_database_image() -> Option<String> {
    let output = docker(&["images", "--format", "{{.Repository}}:{{.Tag}}"])?;
    let listed = String::from_utf8(output.stdout).ok()?;

    let mut images: Vec<&str> = listed
        .lines()
        .map(str::trim)
        .filter(|image| {
            !image.contains("<none>")
                && (image.starts_with("mysql:") || image.starts_with("mariadb:"))
        })
        .collect();
    images.sort_unstable();
    images.first().map(|image| (*image).to_string())
}

/// Start a container of `image` publishing an ephemeral host port.
///
/// The host side of the mapping is left to the daemon on purpose: that is the
/// case a compose file cannot state, and the reason discovery reads the
/// running container rather than the declared configuration.
fn start(image: &str, name: &str) -> Option<Container> {
    let output = docker(&[
        "run",
        "--detach",
        "--name",
        name,
        "--publish",
        "127.0.0.1::3306",
        "--env",
        "MARIADB_DATABASE=shop",
        "--env",
        "MYSQL_DATABASE=shop",
        "--env",
        "MARIADB_USER=reader",
        "--env",
        "MYSQL_USER=reader",
        "--env",
        "MARIADB_PASSWORD=hunter2",
        "--env",
        "MYSQL_PASSWORD=hunter2",
        "--entrypoint",
        "sleep",
        image,
        "120",
    ])?;

    output.status.success().then(|| Container {
        name: name.to_string(),
    })
}

/// The host port docker published for the container's 3306.
fn published_port(name: &str) -> String {
    let output = docker(&["port", name, "3306/tcp"]).expect("docker port runs");
    let mapping = String::from_utf8(output.stdout).expect("docker writes UTF-8");
    mapping
        .lines()
        .next()
        .and_then(|line| line.rsplit(':').next())
        .expect("a published port")
        .trim()
        .to_string()
}

fn dbctx(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dbctx"))
        .args(args)
        .env_clear()
        .output()
        .expect("dbctx binary runs")
}

#[test]
fn a_running_container_is_discovered_through_docker() {
    if !docker_available() {
        eprintln!("skipping: no docker daemon");
        return;
    }
    let Some(image) = local_database_image() else {
        eprintln!("skipping: no mysql or mariadb image pulled");
        return;
    };

    let name = format!("dbctx-discovery-{}", std::process::id());
    let Some(container) = start(&image, &name) else {
        eprintln!("skipping: could not start {image}");
        return;
    };
    let port = published_port(&container.name);

    let output = dbctx(&["-vv", "inspect", "--docker-container", &container.name]);
    let logged = String::from_utf8(output.stderr).expect("dbctx writes UTF-8");

    // Discovery resolves the connection from the running container; inspect
    // then fails because the container is not actually running a database.
    assert_eq!(output.status.code(), Some(2), "{logged}");
    assert!(logged.contains("discovered connection"), "{logged}");
    assert!(logged.contains(&format!("port={port}")), "{logged}");
    assert!(logged.contains("database=[\"shop\"]"), "{logged}");
    assert!(logged.contains("user=Some(\"reader\")"), "{logged}");
    assert!(logged.contains("could not connect"), "{logged}");
    assert!(!logged.contains("hunter2"), "{logged}");

    let engine = if image.starts_with("mariadb:") {
        "mariadb"
    } else {
        "mysql"
    };
    assert!(logged.contains(engine), "{logged}");
}

#[test]
fn a_container_docker_does_not_know_is_reported_by_name() {
    if !docker_available() {
        eprintln!("skipping: no docker daemon");
        return;
    }

    let output = dbctx(&["inspect", "--docker-container", "dbctx-no-such-container"]);
    let reported = String::from_utf8(output.stderr).expect("dbctx writes UTF-8");

    assert_eq!(output.status.code(), Some(3), "{reported}");
    assert!(
        reported.contains("no container named `dbctx-no-such-container`"),
        "{reported}"
    );
}

#[test]
fn a_directory_with_no_compose_project_says_so() {
    if !docker_available() {
        eprintln!("skipping: no docker daemon");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dbctx"))
        .args(["inspect", "--compose-service", "db"])
        .current_dir(dir.path())
        .env_clear()
        .output()
        .expect("dbctx binary runs");
    let reported = String::from_utf8(output.stderr).expect("dbctx writes UTF-8");

    assert_eq!(output.status.code(), Some(3), "{reported}");
    assert!(
        reported.contains("no compose project in this directory"),
        "{reported}"
    );
}
