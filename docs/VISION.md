# VISION

## Mission

Create the industry-standard, vendor-neutral representation of
relational database context for humans and AI coding agents.

## Vision

A developer should be able to understand an unfamiliar database by
running:

``` bash
dbctx inspect
```

The resulting artifacts should be sufficient for documentation,
onboarding, code review, CI, and AI-assisted development.

## Principles

### Facts First

Default output contains only verified database metadata.

### AI Is Optional

Inference, summaries, and narratives are opt-in features and are clearly
identified.

### Deterministic

Identical schemas produce identical artifacts (excluding timestamps and
generator metadata).

### Stable Formats

Document formats are versioned independently from application releases.

### Human Readable

Generated Markdown should be useful even without AI tooling.

### AI Agnostic

Outputs should work equally well with current and future AI coding
agents.

## Architecture Philosophy

    Database
        │
        ▼
    Facts Layer
        │
        ▼
    Analysis Layer
        │
        ▼
    AI Layer

Each layer depends only on lower layers.

## Non-Goals

-   Database migrations
-   ORM generation
-   Query optimization
-   Schema editing
-   Visual modeling tools
-   SQL execution (except the read-only `execute-statement` command introduced in v1.0.0)

## Success Criteria

-   Understand new databases in minutes.
-   Produce version-controlled documentation.
-   Supply trustworthy AI context.
-   Integrate easily into CI/CD.
-   Provide a stable public format for downstream tooling.

## Governance

Project direction is defined by:

1.  VISION.md --- philosophy
2.  SPEC.md --- current implementation contract
3.  RFC/ --- proposed future changes

New functionality should normally begin as an RFC before entering
SPEC.md.

## Long-Term Goal

Make `dbctx inspect` the default answer to:

> "How do I give my AI coding agent an accurate understanding of my
> database?"

## License

MIT OR Apache-2.0.
