# Function Map

Addresses are from the current IDA database.

## Main Save Path

| Address | Working name | Role |
|---|---|---|
| `0x1400DE560` | `guerilla_save_tag_or_save_as` | UI/editor save wrapper. Handles optional rename, save state notifications, dirty-state clearing. |
| `0x140251E50` | `tag_save_current_path` | Loads `tag->path` and calls save core. |
| `0x140251E80` | `tag_save_core` | Main save implementation: read-only checks, stream validation, group pre-save callback, backend write dispatch, cleanup. |
| `0x1402624C0` | `tag_backend_save_dispatch` | Calls active backend table entry at offset `+0x60`. |
| `0x1402C8B00` | `backend_save_tag_to_create_backend` | Concrete save wrapper used by `single_new` and `monolithic_new` table slot `+0x60`. |
| `0x1402C8910` | `write_tag_and_streams_to_create_backend` | Opens create backend writer, writes main tag and alternate streams, commits or aborts. |
| `0x1402C77B0` | `persist_tag_group_data_to_stream` | Wraps output in `c_tag_group_files_persist_context` and calls generic persist walker. |
| `0x14045EAE0` | `tag_group_files_persist_walk` | Generic group-file persist operation for main tag or stream payload. |
| `0x140251D10` | `tag_rename_for_save_as` | Save-as rename/preflight. |
| `0x1400DE600` | `tag_set_dirty_state` | Adds/removes tag from dirty set. Called with `0` after successful save. |
| `0x1400DE720` | `cache_saved_tag_file_timestamp_or_path` | Rebuilds/caches file path or timestamp metadata after save. |
| `0x1400DD1A0` | `notify_tag_state_observers` | In-process observer notification. Called with state `2` before save and `3` after save. |
| `0x1400DDCB0` | `is_tag_dirty` | Checks whether a tag index is in the dirty set. |

## Lower Tag Backend Wrappers

All of these check `byte_14338DBFC`, then dispatch through `qword_14338DBF0`.

| Address | Backend offset | Observed role |
|---|---:|---|
| `0x140261ED0` | `+0x20` | Used by save core to decide if saving over this group/path is blocked/read-only. |
| `0x140261CA0` | `+0x38` | Used by file dialog/open checks; likely path exists / valid tag file check. |
| `0x140261D00` | `+0x48` | Builds a backend path for a group/path. Used after save by `sub_1400DE720`. |
| `0x140262440` | `+0x58` | Load/read dispatch. Called by `sub_14024FDC0`. |
| `0x1402624C0` | `+0x60` | Save/write dispatch. Called by `sub_140251E80`. |
| `0x140261FC0` | `+0xB8` | Returns whether alternate tag streams are active. |
| `0x140262010` | `+0xD0` and `+0x08` | Backend shutdown/clear. |
| `0x1402622E0` | setup | Replaces active backend and calls backend startup/shutdown hooks. |

## Concrete Backend Tables / Writers

| Address | Working name | Role |
|---|---|---|
| `0x14118B1E0` | `single_new_backend_table` | Active backend table for `"single_new"`. Slot `+0x60` is `sub_1402C8B00`. |
| `0x14118B300` | `monolithic_new_backend_table` | Active backend table for `"monolithic_new"`. Slot `+0x60` is also `sub_1402C8B00`. |
| `0x140D1A320` | `c_single_tag_file_create_backend_vftable` | Create/write backend vtable. `+0x08` open, `+0x10` commit, `+0x18` abort. |
| `0x1402CB860` | `single_create_backend_open_writer` | Opens temporary writer/stream for save. |
| `0x1402CCBF0` | `single_create_backend_commit_writer` | Finalizes header and overwrites destination under `tags`. |
| `0x1402CB170` | `single_create_backend_abort_writer` | Discards active writer/temp data on failure. |
| `0x140470FF0` | `single_tag_file_writer_ctor_and_header` | Initializes writer and writes initial tag header. |
| `0x140DBEBC8` | `c_single_tag_file_writer_vftable` | Writer vtable. Main stream, named stream, close/commit chunk. |
| `0x140471B20` | `single_writer_open_main_stream` | Opens main stream fourcc `1952540449`. |
| `0x1404718D0` | `single_writer_open_named_stream` | Opens alternate data stream. |
| `0x1404716E0` | `single_writer_commit_current_chunk` | Commits current data chunk; stores main checksum/header state. |
| `0x1404715A0` | `single_writer_finalize_header` | Seeks back and writes final tag file header. |

## Reload / Sapien-Relevant Path

| Address | Working name | Role |
|---|---|---|
| `0x140251AC0` | `tag_reload_existing_or_load` | Reloads an already-open tag from disk/backend and notifies in-process observers. |
| `0x14024FDC0` | `tag_load_core` | Core load/reload path. Logs resync/xsync errors. Calls backend load dispatch `sub_140262440`. |
| `0x140252E90` | `dispose_tag_blocks_and_streams` | Deletes old tag block elements and releases stream/cache resources. |
| `0x1404698F0` | `monolithic_reload_from_pending_xsync` | Checks for `xsync_write_lock`, then reloads tag cache from pending sync. |
| `0x1402692C0` | `single_tag_xsync_init` | Initializes xsync helper using tag root path from backend offset `+0x40`. |
| `0x140269080` | `single_tag_xsync_difference_callback` | Parses changed path, compares checksum/read-only state, marks reload bitset, sets pending flag. |
| `0x140269330` | `single_tag_xsync_iterate_differences` | Iterates changed paths and calls `sub_140269080`. |
| `0x140269360` | `single_tag_xsync_clear_pending` | Clears pending reload flag and bitset. |
| `0x1402695D0` | `xsync_xbox_connect` | Devkit/Xbox connection helper. Logs `tags:xsync: DmOpenConnection failed`. |

## Important Globals / Data

| Address | Meaning |
|---|---|
| `qword_14338D988` | Tag datum array/table base used by `sub_14030CD50`. |
| `qword_14338DBF0` | Active tag backend/interface table. |
| `byte_14338DBFC` | Tag backend initialized flag. |
| `off_141171418` | Backend setup callback/table used during tag initialization. |
| `off_14118B1E0` | `single_new` backend table; save slot `+0x60` is `sub_1402C8B00`. |
| `off_14118B300` | `monolithic_new` backend table; save slot `+0x60` is `sub_1402C8B00`. |
| `qword_1433A35D0` | Concrete backend aggregate/context used by `sub_1402C8B00`; field `+24` is create/write backend. |
| `qword_1433A35E0` | Allocator/context pointer used in backend writer context. |
| `qword_14116F6C8[4]` | Up to four tag reload observers notified after successful reload in `sub_140251AC0`. |
| `byte_14116F6F6` | Observer system initialized flag checked before notifying reload observers. |

## Useful Strings

Save:

- `tags: cannot save tag '%s.%s'; missing tag streams`
- `tags: cannot save over the read-only tag '%s.%s' with '%s.%s'`
- `tags: cannot save over the read-only tag '%s.%s'`
- `tags: can't rename '%s' to '%s' because a tag with that name is already open.`
- `tags: can't rename '%s.%s' because it is using the tag file cache.`

Reload/xsync:

- `tags:reloading:`
- `tags:reloading: failed to reload %s`
- `reloading tag cache from pending sync...`
- `tags:monolithic: reloading from sync`
- `tags:monolithic: index file has not been updated for current sync`
- `tags:loading: tag needs to be resynced (via xsync): '%s.%s'.`
- `xsync_write_lock`
