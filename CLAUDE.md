# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Rust-based `kubectl` plugin that displays resource allocations (CPU, memory, GPU, etc.) across Kubernetes clusters. It shows requested, limit, allocatable, and utilization metrics without displaying usage like `kubectl top`. The plugin can group results by namespaces, nodes, pods, and filter by resource names.

## Architecture

### Core Components
- **lib.rs**: Main library containing core logic for resource collection, grouping, and display
- **main.rs**: CLI entry point with argument parsing and initialization
- **metrics.rs**: Kubernetes Metrics API integration for utilization data
- **qty.rs**: Quantity handling and unit conversion utilities
- **tree.rs**: Tree structure utilities for hierarchical display

### Key Data Structures
- `Resource`: Represents a resource with kind, quantity, location, and qualifier (Limit/Requested/Allocatable/Utilization)
- `Location`: Contains node_name, namespace, and pod_name for resource grouping
- `QtyByQualifier`: Aggregates quantities by qualifier type for display calculations

### Resource Collection Flow
1. Collect allocatable resources from nodes
2. Collect resource requests/limits from scheduled pods
3. Optionally collect utilization metrics from Metrics API
4. Group resources by specified criteria (resource, node, pod, namespace)
5. Display results in table or CSV format

## Development Commands

This project uses **mise** for development tool management and requires **Rust nightly** toolchain for edition2024 support.

### Toolchain Setup
```bash
rustup update nightly       # Install/update nightly toolchain with edition2024 support
```

### Building
```bash
cargo +nightly build        # Build the project using nightly toolchain
mise run build              # Build the project (with clean) using mise
mise run build-release      # Build in release mode
mise run build-release-for-target  # Cross-platform build
```

### Testing and Quality
```bash
cargo +nightly test         # Run tests using nightly toolchain
cargo +nightly clippy -- -D warnings # Lint with clippy
cargo +nightly fmt          # Format code
mise run ci                 # Main CI flow: check, test, clippy (requires mise)
```

### Local Development
```bash
cargo +nightly run          # Run the plugin locally
cargo +nightly run -- --help # Show CLI options
```

## Usage Examples

### Filter by Node Taints
```bash
# Default: Only show nodes without taints (workload nodes)
kubectl-view-allocations

# Ignore all taints and show all nodes (including control-plane)
kubectl-view-allocations --ignore-taints

# Show untainted nodes + nodes with specific taints (ignore these taints)
kubectl-view-allocations --ignore-taints node-role.kubernetes.io/control-plane

# Show untainted nodes + nodes with specific taint key-value pairs
kubectl-view-allocations --ignore-taints dedicated=database

# Ignore multiple taint patterns
kubectl-view-allocations --ignore-taints node-role.kubernetes.io/control-plane,dedicated=database

# Combine with other filters
kubectl-view-allocations -l environment=production --ignore-taints dedicated=database

# Support taints literally named 'any'
kubectl-view-allocations --ignore-taints any

# Common use cases:
# - Default: Show only workload nodes (no taints)
# --ignore-taints: Show all nodes for complete cluster overview
# --ignore-taints node-role.kubernetes.io/control-plane: Show workload + control-plane nodes
# --ignore-taints dedicated,workload: Include dedicated/workload nodes with workload nodes
```

### Kubernetes Testing
```bash
mise run k8s_create_kind    # Create kind cluster with metrics-server
mise run k8s_delete_kind    # Delete kind cluster
mise run k8s_create_kwok    # Create KWOK cluster for testing
mise run k8s_delete_kwok    # Delete KWOK cluster
```

### Release and Distribution
```bash
mise run zip-release-ci-flow    # Build and package for distribution
mise run publish                # Publish to crates.io
```

## Environment Variables

Key environment variables from `.mise.toml`:
- `CLUSTER_NAME="demo-kube"`: Default cluster name for testing
- `TARGET_AUTO="x86_64-unknown-linux-gnu"`: Default build target
- `RUST_TEST_THREADS="1"`: Single-threaded tests for Kubernetes API calls

## Features

- **default**: Enables CLI features with tokio runtime and prettytable display
- **prettytable**: Table formatting for CLI output
- **k8s-openapi/v1_20**: Kubernetes API support (can be customized)

## Dependencies

- **kube**: Kubernetes client library with TLS and auth support
- **k8s-openapi**: Kubernetes API types
- **clap**: CLI argument parsing with derive macros
- **prettytable-rs**: CLI table formatting (optional)
- **itertools**: Iterator utilities for grouping operations
- **tokio**: Async runtime for Kubernetes API calls

## Testing

Integration tests run against real Kubernetes clusters using kind or KWOK. The project includes test manifests for metrics-server setup in `tests/metrics-server-components.yaml`.

## Build Targets

The project supports cross-compilation for multiple targets using the `cross` tool. Release builds are optimized for size with LTO enabled.