# Repository conventions

Treat all project content as public.

- Keep source, documentation, comments, examples, tests, and configuration free of
  personal identifiers, machine-specific absolute paths, private hostnames,
  internal project references, credentials, and environment dumps. The copyright
  attribution in LICENSE is intentionally public and is exempt from this rule.
- Use relative paths and generic examples. Shared dependencies use published
  versions; local overrides belong in untracked Cargo configuration.
- Describe the software and its behavior for an outside contributor. Keep private
  development history and conversation-specific instructions out of public docs.
- Preserve the existing project copyright attribution in LICENSE and required
  third-party license and copyright notices.
- Keep build artifacts, local environment settings, and machine-local Cargo
  configuration out of version control.
- Review changed content for accidental personal or internal information before
  delivering changes.
- Keep user-facing behavior in README.md (written for users first), the
  contributor workflow in CONTRIBUTING.md, module boundaries and output contracts
  in docs/architecture.md, planned feature decisions in docs/feature-design.md,
  and remaining implementation work in docs/roadmap.md.
- Preserve docs/performance-baseline.md and its raw samples as a fixed reference.
  Measure feature costs and unchanged paths before accepting expensive rendering
  changes. Keep large benchmark inputs, generated output, and new local results
  outside the repository; bench/ contains only small tooling and reviewed data.
