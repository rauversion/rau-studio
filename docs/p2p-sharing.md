# P2P Sharing Foundation

Rau Connect is the foundation for peer identity, contacts, presence, shared-folder catalogs, chat, and peer-to-peer downloads. The local security and catalog boundaries remain separate from the transport, but Rau can now start an authenticated Iroh endpoint and exchange diagnostic traffic with another running instance.

## Current Scope

Implemented:

- one Ed25519 device identity;
- password-based identity unlocking;
- Argon2id key derivation with versioned parameters;
- AES-256-GCM private-key encryption inside SQLite;
- in-memory unlocked identity that is cleared when locked or when the process exits;
- persistence tables for trusted peers and presence state;
- read-only shared-folder definitions;
- recursive catalog indexing that ignores hidden entries and symlinks;
- virtual relative paths that do not expose the local absolute path to peers;
- stable opaque file IDs scoped to each share;
- local catalog search using the same response shape intended for remote queries;
- pause, reindex, and remove operations that never modify original files;
- Iroh endpoint lifecycle backed by the decrypted device identity;
- direct QUIC connectivity with relay fallback through the Iroh N0 preset;
- versioned `/rau/diagnostic/1` request/response traffic;
- shareable endpoint tickets, authenticated peer IDs, and measured round-trip time;
- observed peer persistence and a two-minute recent-reachability presence lease;
- Tauri network events and React controls for start, stop, ticket copy, connection test, and the online/offline device list.

Not yet implemented:

- QR rendering, one-use pairing invitations, and contact authorization;
- periodic presence heartbeats;
- private or general chat;
- remote catalog request handling;
- resumable file download;
- community discovery and moderation.

## Identity Storage

The device generates a 32-byte Ed25519 seed. Its public key becomes the stable endpoint ID. The private seed is never stored as plaintext.

```text
password + random salt
        |
        v
Argon2id
        |
        v
256-bit wrapping key
        |
        v
AES-256-GCM(device private seed, endpoint ID as AAD)
        |
        v
SQLite p2p_identity
```

The endpoint ID is included as authenticated associated data. Moving ciphertext to a different endpoint record therefore fails authentication. The salt, nonce, KDF parameters, and cipher version are non-secret and are stored alongside the ciphertext.

Rau cannot recover a forgotten P2P password. A future recovery/export flow must explicitly re-encrypt the identity with a recovery key or replacement password.

## Shared Folder Boundary

A share grants catalog and download access to a virtual read-only root. It does not grant arbitrary filesystem access.

```text
Local path:    /Users/alicia/Music/Masters/House/Night Drive.aiff
Share root:    /Users/alicia/Music/Masters
Remote path:   House/Night Drive.aiff
Remote file:   SHA256(share_id || 0x00 || remote_path)
```

The catalog excludes hidden paths and symlinks. Before a future download begins, the backend must resolve the selected file again, verify that it remains a regular file under the canonical share root, re-check the share ACL, and bind the transfer to an immutable content hash.

## SQLite Tables

- `p2p_identity`: encrypted device identity and versioned KDF parameters.
- `p2p_peers`: paired endpoint IDs, trust, last address, and last presence observation.
- `p2p_shares`: local roots, visibility policy, counters, and enabled state.
- `p2p_shared_files`: opaque IDs and virtual metadata for indexed files.

Visibility values are intentionally bounded:

- `contacts`
- `selected_contacts`
- `community`
- `ticket`

`selected_contacts` will be backed by a share ACL table when pairing is implemented. `ticket` will be backed by hashed, expiring capability tokens.

## Tauri Commands

Identity:

- `p2p_identity_status`
- `p2p_create_identity`
- `p2p_unlock_identity`
- `p2p_lock_identity`

Catalog:

- `p2p_list_shares`
- `p2p_add_share`
- `p2p_reindex_share`
- `p2p_set_share_enabled`
- `p2p_remove_share`
- `p2p_search_shared_files`

Peers:

- `p2p_list_peers`

Network:

- `p2p_network_status`
- `p2p_network_start`
- `p2p_network_stop`
- `p2p_network_ping_ticket`

## Network Handshake

The current network flow proves transport and identity before catalog or chat permissions are added:

```text
Device A ticket
      |
      v
Iroh connect(ALPN /rau/diagnostic/1)
      |
      +-- QUIC authenticates Device B endpoint ID
      |
      v
bounded JSON ping(nonce, version, public display name)
      |
      v
bounded JSON pong(same nonce, B endpoint ID, display name)
      |
      v
A verifies pong endpoint ID == authenticated QUIC peer ID
      |
      v
peer observation + RTT + p2p-network-event
```

The endpoint ticket is public connection metadata, not the private key. The future QR screen will encode this ticket together with a short-lived, one-use invitation capability. Scanning a raw diagnostic ticket currently proves which endpoint answered, but does not yet make that endpoint a trusted contact.

`online` currently means that the peer completed an authenticated diagnostic exchange during the last two minutes. It automatically appears `offline` after the lease expires. Periodic heartbeat traffic will renew this lease in a later slice.

## Next Network Slice

The next slice can build on the verified transport without changing the local catalog shapes:

1. Add `/rau/pair/1`, expiring one-use invitations, QR rendering, and explicit acceptance.
2. Promote accepted endpoint IDs from `observed` to `paired` in `p2p_peers`.
3. Add periodic presence heartbeats for paired contacts.
4. Add `/rau/catalog/1` with bounded, authorized search requests.
5. Resolve a result to a revalidated file handle and content hash.
6. Add the download transport behind a `BlobTransport` boundary.
7. Add `/rau/chat/1` only after peer authorization and delivery acknowledgements are stable.

The public general room should remain a separate protocol and policy boundary from trusted private contacts.
