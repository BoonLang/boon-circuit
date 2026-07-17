# FjordPulse Coolify Runbook

This directory defines the production image and Coolify process contract. It is
not evidence that FjordPulse has been deployed, that Live provider integration
passes, or that Phase 12 is complete.

## Image Contract

Build from the repository root and an exact source revision:

```bash
docker build \
  --build-arg SOURCE_REVISION="$(git rev-parse HEAD)" \
  -f deploy/fjordpulse/Dockerfile \
  -t fjordpulse-boon:"$(git rev-parse HEAD)" .
```

The build stage uses the locked Cargo graph to compile the generic package CLI,
the generic static server, the browser WebGPU host, and the two immutable Boon
artifacts. The final `scratch` image contains only the static server binary,
the closed package inventory, and CA certificates. It runs as UID/GID 65532,
has no shell, compiler, package manager, Python, Node, PHP, or database server,
and writes only to `/var/lib/boon/fjordpulse` plus bounded `/tmp` scratch space.

`bundle.cbor` is the runtime authority for source revision, compiler identity,
artifact/content/plan digests, roles, capability profiles, protocol version,
state namespaces, and every packaged file digest. Startup fails before binding
port 8080 when any value or byte differs.

## Coolify Resource

1. Create one Docker Compose resource from `compose.coolify.yaml`.
2. Configure exactly one replica and disable overlapping rollout against the
   same volume.
3. Mount one persistent Coolify volume at `/var/lib/boon/fjordpulse`.
4. Route only container port 8080 through Coolify's managed proxy. Do not add a
   public host port, custom Docker network, or database port.
5. Configure `fjordpulse-boon.kavik.cz`, HTTPS redirect, TLS, and WSS.
6. Set every required value from `coolify.env.example` in Coolify. Secret values
   and secret references must not be committed to this repository or image.
7. Use `/api/readiness` for rollout readiness and `/api/health` for ongoing
   process liveness. Set the graceful stop timeout to at least 45 seconds.

Live startup is intentionally fail-closed: a deterministic bundle/mode, HTTP
public origin, wrong state namespace, malformed trusted proxy list, missing
provider setting, absent secret reference, artifact mismatch, or second writer
against the volume prevents readiness.

## Evidence Sequence

Deployment is permitted only after the exact source revision has fresh passing
deterministic, contract, browser, persistence, security, architecture, image,
and performance reports required by the canonical FjordPulse plan.

1. Deploy the exact image to a staging hostname and fresh staging volume.
2. Run black-box HTTPS/WSS and image smokes through the public proxy.
3. Create sentinel state through normal Boon workflows.
4. Restart the process and container; compare sentinel and schema digests.
5. Redeploy the exact image against the same volume; compare again.
6. Exercise a compatible migration and a forced migration failure in staging.
7. Add Netlify DNS for `fjordpulse-boon.kavik.cz` without altering the existing
   `fjordpulse.kavik.cz` deployment.
8. Deploy the gated SHA and verify map, search, station, vehicle, Focus, Admin,
   health, readiness, HTTPS, WSS, security headers, Entur identity, and raster
   provider identity.
9. Verify the existing production hostname remains independent.

The persistent volume preserves ordinary restart/redeploy continuity. It is not
a backup and does not protect against host or disk loss, volume deletion, or
operator error. Backup/restore automation remains a separately deferred item.
