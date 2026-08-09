# Reachability test — does NAT traversal actually work?

Settles [open decision 2](./OPEN-DECISIONS.md#2-does-nat-traversal-actually-work).
M2 demonstrated identity-only addressing with both nodes on one machine, which
proves a peer can be dialled by endpoint id — but says nothing about NAT, since
no relay was necessarily involved. This needs two machines on **different
networks**.

Allow about fifteen minutes.

---

## Before you start

**The away machine must not share a network with the home machine.** A phone
hotspot is ideal. On the same wifi the two will find a direct path over the LAN,
which proves nothing about the thing being tested — and the run will *look* like
a success. This is the one way to get a false pass.

**`KAMIROH_REACH=anywhere` publishes.** Both nodes sign a record of their
addresses and publish it to n0's lookup service under their endpoint ids; anyone
holding an id can then resolve where the node is. That is
[ARCHITECTURE.md §5a](./ARCHITECTURE.md), and it is the trade this test
exercises. The keys below are throwaway, so the records are keyed to ephemeral
ids and expire on their own.

**Both machines need the binary.** `cargo build --release -p kamiroh`, or copy
`target/debug/kamiroh` across. The commands below call it `$KAMIROH`, so set
that once on each machine and the rest can be pasted as-is:

```bash
export KAMIROH=./kamiroh                 # a copied binary
export KAMIROH=~/src/kamiroh/target/release/kamiroh   # or a built one
```

The agents are `echo` throughout. This test is about the network; using a real
coding agent would spend tokens to prove nothing extra.

---

## 1. Learn each node's id

On **each** machine, in its own directory:

```bash
mkdir -p ~/kamiroh-test && cd ~/kamiroh-test
KAMIROH_KEY_FILE=./node.key KAMIROH_ALLOW_FILE=./allow \
  "$KAMIROH" < /dev/null 2>/dev/null | grep '^endpoint id:'
```

It prints an id and keeps running; Ctrl-C once you have it. The key is written
to `./node.key`, so the same id comes back on every later run.

Now give **each machine the other one's id** — this is the only thing that has
to travel between them, and it is the step that is easy to do backwards:

```bash
# on the HOME machine, holding the away machine's id
export AWAY_ID=<the id printed on the away machine>

# on the AWAY machine, holding the home machine's id
export HOME_ID=<the id printed on the home machine>
```

Ids are public keys, so sending them over any channel you like is fine — that is
what makes them safe to paste. What they are *not* is a secret that admits
anyone: the allowlist still decides.

---

## 2. Home machine — admit the away node, and publish

```bash
cd ~/kamiroh-test
echo "$AWAY_ID" > ./allow          # only the away node may drive this one

KAMIROH_KEY_FILE=./node.key \
KAMIROH_ALLOW_FILE=./allow \
KAMIROH_REACH=anywhere \
  "$KAMIROH"
```

Leave it running. You should see:

```
reach:       n0 relays + address lookup — this node's addresses are published
allowing:    1 peer(s) from ./allow
serving — press Ctrl-C to stop
```

---

## 3. Away machine — dial by identity alone

Note there is **no address anywhere in this command**. That is the point.

```bash
cd ~/kamiroh-test
KAMIROH_KEY_FILE=./node.key \
KAMIROH_ALLOW_FILE=./allow \
KAMIROH_REACH=anywhere \
KAMIROH_PEER="$HOME_ID" \
  "$KAMIROH"
```

Then type at the prompt:

```
hello from the other end
```

---

## 4. What counts as proof

**On the away machine**, the console echoes your line back. That reply travelled
to the home machine and back, addressed by identity only.

**On the home machine**, stderr shows how the peer arrived:

```
INFO kamiroh_adapter_iroh::front: accepted a peer peer=… path=relayed via https://…
INFO kamiroh_adapter_iroh::front: accepted a peer peer=… path=direct 203.0.113.7:41234
```

**Either line settles the decision.** A relayed path and a hole-punched direct
one are both things a bare `host:port` could never have achieved from behind
NAT. The difference matters for a different reason: a relayed path means a third
party is carrying the traffic — encrypted, and seeing only which endpoints talk,
when, and how much (§5a) — while a direct path means the two machines found each
other. Iroh may also start relayed and upgrade to direct; the line is a snapshot
taken when the connection arrived.

**What would be a false pass:** `path=direct 192.168.x.x` or `10.x.x.x`. Those
are private addresses, so the machines were on one network after all. Move the
away machine to a hotspot and run it again.

---

## 5. If it does not work

- **`unreachable: no address for this peer…`** — `KAMIROH_REACH` is not
  `anywhere` on the away machine. That error names the fix.
- **`refused the connection`** — the two nodes found each other, so
  **reachability already works**; the home node just does not admit this id.
  Check `./allow` on the home machine matches `AWAY_ID` exactly. Worth noting
  this is still a pass for the decision: refused means found.
- **Nothing at all, and the prompt hangs** — the home node's record may not have
  propagated yet. Give it a minute after starting the home node before dialling.
- **`timed out waiting for a reply`** — reached and then stalled. Worth
  recording rather than retrying blindly; it is more interesting than a clean
  failure.

---

## 6. Afterwards

Record the outcome in [OPEN-DECISIONS.md](./OPEN-DECISIONS.md) — including which
path type you got — and strike item 2. A decision leaves that list by being
decided, not by going quiet.

If it fails, that is a finding rather than a setback: the README's headline case
would then be aspirational, and phase 3 should say so before anyone else is
invited to rely on it.
