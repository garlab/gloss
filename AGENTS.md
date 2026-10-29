# Repository conventions

Treat all project content as public.

- Keep source, documentation, comments, examples, tests, and configuration free of
  personal identifiers, machine-specific absolute paths, private hostnames,
  internal project references, credentials, and environment dumps. The copyright
  attribution in LICENSE is intentionally public and is exempt from this rule.
- Use relative paths and generic examples. The local `../katex-native` dependency
  is intentional until a public dependency is available; do not replace it with an
  absolute checkout path.
- Describe the software and its behavior for an outside contributor. Keep private
  development history and conversation-specific instructions out of public docs.
- Preserve the existing project copyright attribution in LICENSE and required
  third-party license and copyright notices.
- Keep build artifacts, local environment settings, and machine-local Cargo
  configuration out of version control.
- Review changed content for accidental personal or internal information before
  delivering changes.
- Keep user-facing behavior in README.md, module boundaries and output contracts
  in docs/architecture.md, and remaining implementation work in docs/roadmap.md.
