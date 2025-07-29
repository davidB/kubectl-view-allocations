# Migration from cargo-make to mise

This document describes the migration from `cargo-make` to `mise` for task management in the kubectl-view-allocations project.

## Overview

The project has been migrated from using `cargo-make` with `Makefile.toml` to using `mise` with tasks defined in `.mise.toml`. This migration provides better integration with the development environment and eliminates the need for an external task runner dependency.

## Task Mapping

### Basic Development Tasks

| cargo-make (old) | mise (new) | Description |
|------------------|------------|-------------|
| `cargo make format` | `mise run format` | Format code with rustfmt |
| `cargo make clean` | `mise run clean` | Clean build artifacts |
| `cargo make build` | `mise run build` | Build the project |
| `cargo make test` | `mise run test` | Run tests |
| `cargo make check` | `mise run check` | Check the project for errors |
| `cargo make clippy` | `mise run clippy` | Run clippy linter |

### CI/Build Tasks

| cargo-make (old) | mise (new) | Description |
|------------------|------------|-------------|
| `cargo make ci-flow` | `mise run ci-flow` | Main CI flow - format, check, test, clippy |
| `cargo make ci-static-code-analysis-tasks` | `mise run ci-static-code-analysis-tasks` | Static code analysis tasks for CI |
| `cargo make build-release-for-target` | `mise run build-release-for-target` | Build release for specific target |

### Release Tasks

| cargo-make (old) | mise (new) | Description |
|------------------|------------|-------------|
| `cargo make zip-release-ci-flow` | `mise run zip-release-ci-flow` | Complete release build and packaging |
| `cargo make zip-release-binary-for-target` | `mise run zip-release-binary-for-target` | Create release archive for target |
| `cargo make dist_env` | `mise run dist_env` | Set up distribution environment variables |

### Documentation Tasks

| cargo-make (old) | mise (new) | Description |
|------------------|------------|-------------|
| `cargo make update-changelog` | `mise run update-changelog` | Update changelog using gitmoji-changelog |
| `cargo make update-bom` | `mise run update-bom` | Update Bill of Materials |
| `cargo make update-docs` | `mise run update-docs` | Update all documentation |

### Publishing Tasks

| cargo-make (old) | mise (new) | Description |
|------------------|------------|-------------|
| `cargo make pre-publish` | `mise run pre-publish` | Pre-publish tasks |
| `cargo make publish` | `mise run publish` | Publish to crates.io |

### Kubernetes Testing Tasks

| cargo-make (old) | mise (new) | Description |
|------------------|------------|-------------|
| `just k8s_create_kind` | `mise run k8s_create_kind` | Create kind cluster with metrics-server |
| `just k8s_delete_kind` | `mise run k8s_delete_kind` | Delete kind cluster |
| `just k8s_create_kwok` | `mise run k8s_create_kwok` | Create KWOK cluster with test pods |
| `just k8s_delete_kwok` | `mise run k8s_delete_kwok` | Delete KWOK cluster |

### Utility Tasks

| cargo-make (old) | mise (new) | Description |
|------------------|------------|-------------|
| `cargo make debug` | `mise run debug` | Print debug information |
| `cargo make default` | `mise run default` | List available tasks |

## Key Changes

### 1. Removed Dependencies

- **cargo-make**: No longer needed as a development dependency
- **just**: Replaced with mise's native task runner
- **Makefile.toml**: Removed in favor of `.mise.toml` configuration

### 2. Added Dependencies

- **jq**: Added as a mise tool for JSON processing in release tasks
- **mise**: Now used as the primary task runner

### 3. Environment Variables

Environment variables are now defined in the `[env]` section of `.mise.toml`:

```toml
[env]
CLUSTER_NAME = "demo-kube"
DOCKER_BUILDKIT = "1"
RUST_TEST_THREADS = "1"
TARGET_AUTO = "x86_64-unknown-linux-gnu"
LIBZ_SYS_STATIC = "1"
PKG_CONFIG_ALLOW_CROSS = "1"
OPENSSL_STATIC = "1"
```

### 4. Task Dependencies

- **Sequential execution**: Tasks like `ci-flow` now run commands sequentially to avoid filesystem conflicts
- **Simplified dependencies**: Dependencies are handled through explicit command sequences rather than cargo-make's dependency system

### 5. GitHub Actions Integration

The GitHub workflows have been updated to use `mise` instead of `cargo-make`:

**Before:**
```yaml
- uses: davidB/rust-cargo-make@v1
- run: cargo make --disable-check-for-updates ci-flow
```

**After:**
```yaml
- uses: jdx/mise-action@v2
- run: mise run ci-flow
```

## Benefits of Migration

1. **Unified tooling**: Everything is managed through mise (tools + tasks)
2. **Better performance**: No external dependency on cargo-make
3. **Simpler configuration**: Single `.mise.toml` file for both tools and tasks
4. **Native integration**: Better integration with the development environment
5. **Reduced complexity**: Eliminated cargo-make's complex dependency system

## Usage Examples

```bash
# List all available tasks
mise tasks ls

# Run CI flow
mise run ci-flow

# Build release for specific target
TARGET=x86_64-unknown-linux-gnu mise run build-release-for-target

# Create release package
TARGET=x86_64-unknown-linux-gnu mise run zip-release-ci-flow

# Run Kubernetes tests
mise run k8s_create_kind
```

## Migration Verification

All original functionality has been preserved and tested:

- ✅ Basic development tasks (build, test, format, etc.)
- ✅ CI workflows for GitHub Actions
- ✅ Release packaging and distribution
- ✅ Cross-compilation support
- ✅ Environment variable handling
- ✅ Kubernetes testing workflows
- ✅ Documentation generation tasks

The migration maintains full backward compatibility in terms of functionality while providing a more streamlined and integrated development experience.
