# Brief: the Incus check — kamiroh across container boundaries

*2026-08-17. Runbook: `docs/INCUS-CHECK.md`. The spike's deployment-shape
validation, and the last open errand from spike-1's list — run after
graduation, against the canonical repo's tree.*

## Result: CHECK PASSED

Two fresh Debian 13 containers (`kamiroh-a`, `kamiroh-b`) under Incus —
itself hosted inside a Lima VM on an Apple-silicon Mac — each provisioned
from scratch: toolchain, clone of `casey-bowman/kamiroh` (the graduated
tree), release build. The standard proof ran A→B across the container
bridge under the default hermetic profile:

- `PONG 3.42ms` — the container boundary costs almost nothing.
- `SPAWNED echo-incus`, `TURN OK (ack seen: true)` — remote spawn and a
  full turn exchange, with the delivery ack arriving before the answer,
  as decision 4 promises.
- `PATHS` listed exactly one path: the direct bridge address, selected.
  No relay entry anywhere — hermetic means hermetic.

Only B's address was ever configured. A was introduced to B statically
(`--peer-ip`); B learned A entirely from the inbound connection, and its
replies rode that connection back — the symmetric-reader design
(`one_sided_introduction_suffices_for_replies`) doing its job in the
deployment shape it was built for.

Build cost on Apple silicon: ~38.6 s of compilation per container for the
full workspace (270 crates), a few minutes total with apt + rustup +
clone around it.

## Method notes

**Container DNS failed on exactly one hostname.** Fresh containers
resolved deb.debian.org, sh.rustup.rs, and github.com fine — then cargo
died on `index.crates.io`: "Could not resolve host", through all retries,
in both containers. The default resolver chain (container → Incus bridge
dnsmasq → Lima VM → macOS) chokes on that specific CDN-backed record;
cause undiagnosed. Fix that worked instantly:

```sh
echo "nameserver 1.1.1.1" > /etc/resolv.conf   # inside the container
```

after which the name resolved (to Fastly IPv6 addresses) and the build
proceeded. Two caveats for anyone reusing this: the `resolv.conf`
overwrite is ephemeral (DHCP or systemd-resolved may rewrite it on
restart — an Incus profile or resolved drop-in is the durable form), and
"DNS works" is not one fact — diagnose per-hostname with
`getent hosts <name>`. The fallback path, never needed, was the
workshop's `vendor-snapshot` branch and an `--offline` build.

Minor second observation: freshly launched `images:debian/13` containers
carried a pre-existing `/root/.rustup/settings.toml` (rustup warned, then
installed the correct toolchain anyway) — surprising state for "fresh"
containers, harmless here.

## Hygiene

Demo identities from the repo, appropriately: both endpoints lived on a
host-local container bridge, unreachable from outside the machine, and
the containers were deleted after the run (`incus delete -f`).

## What this closes

With the internet check (two networks, direct paths) and now the
container check both passed, every validation on spike-1's list is done:
same binary, same protocol, proven across loopback, containers, and the
public internet.
