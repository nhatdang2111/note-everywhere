//! Size limits enforced by the server and respected by the client.

/// Largest binary WebSocket frame in either direction.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
/// Largest encrypted update (one envelope).
pub const MAX_UPDATE_PAYLOAD: usize = 256 * 1024;
/// Largest encrypted snapshot. Snapshots live in a SQLite row of the Durable
/// Object, whose rows are limited to 2 MB, and must fit one frame.
pub const MAX_SNAPSHOT_PAYLOAD: usize = 960 * 1024;
/// Most entries in one push.
pub const MAX_PUSH_ENTRIES: usize = 100;
/// Total payload bytes a client puts in one push (leaves room for headers).
pub const MAX_PUSH_BYTES: usize = 768 * 1024;
/// Most entries the server returns in one page.
pub const MAX_PAGE_ENTRIES: usize = 200;
/// Longest document, device or workspace identifier.
pub const MAX_ID_LEN: usize = 64;
/// Largest JSON request body of the HTTP API.
pub const MAX_JSON_BODY: usize = 64 * 1024;
/// Largest attachment object the server accepts regardless of configuration.
/// Stays under the 100 MB request body limit of Cloudflare's free plan.
pub const MAX_BLOB_CEILING: u64 = 90 * 1024 * 1024;
/// Default attachment object cap.
pub const DEFAULT_BLOB_MAX: u64 = 64 * 1024 * 1024;
