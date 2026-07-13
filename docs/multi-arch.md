# Multi-arch Docker builds (ARM64 + x86-64)

Digestly must run on both a Raspberry Pi (ARM64) and a typical x86-64 server (prompt.md §1). The
`Dockerfile` is architecture-agnostic - the Node and Rust build stages compile for whatever platform
they run on, and the base images (`node:20-bookworm-slim`, `rust:1.88-slim-bookworm`,
`debian:bookworm-slim`) all publish `linux/amd64` and `linux/arm64` variants - so a single multi-arch
image is produced with `docker buildx`. This is a **CI-level** concern: `docker compose up` on the
target host builds the correct single-arch image locally without any of this.

## One-off multi-arch build + push

```bash
# Create a buildx builder once (uses QEMU emulation for the non-native arch).
docker buildx create --name digestly --use
docker run --privileged --rm tonistiigi/binfmt --install all   # enable emulation

# Build for both architectures and push a single multi-arch tag to a registry.
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t <registry>/digestly:latest \
  --push .
```

`--push` is required for true multi-arch images: a manifest list can't be loaded into the local
Docker image store, only pushed to a registry (or written with `--output type=oci`).

## Building only for the local Pi

On the Pi itself, no buildx is needed - the normal build already targets ARM64:

```bash
docker compose build      # or: docker compose up --build
```

## CI sketch (GitHub Actions)

```yaml
- uses: docker/setup-qemu-action@v3
- uses: docker/setup-buildx-action@v3
- uses: docker/build-push-action@v6
  with:
      context: .
      platforms: linux/amd64,linux/arm64
      push: true
      tags: <registry>/digestly:latest
```

Emulated ARM64 compilation of the Rust stage is slow in CI; a native ARM runner (or
`cargo`-level cross-compilation) is faster if build time becomes a problem.
