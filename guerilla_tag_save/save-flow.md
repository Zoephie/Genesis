# Save Flow

## Normal Save

Entry point seen from UI/editor save prompts:

`sub_1400DE560(int tag_index, const char *optional_new_path)`

Behavior:

1. Rejects `tag_index == -1`.
2. If `optional_new_path` is non-null and different from the current path, calls `sub_140251D10(tag_index, optional_new_path)` to perform a save-as/rename preflight.
3. Calls `sub_1400DD1A0(tag_index, 2)` before saving. This is an observer/state broadcast inside the process.
4. Calls `sub_140251E50(tag_index)`.
5. On success:
   - Calls `sub_1400DE720(tag_index)`.
   - Calls `sub_1400DE600(tag_index, 0)` to remove the tag from the dirty set.
   - Calls `sub_1400DD1A0(tag_index, 3)` after saving.

`sub_140251E50(unsigned int tag_index)` is just a wrapper:

```c
tag = tag_get(tag_index);
return sub_140251E80(tag_index, tag->path);
```

## Save Core

`sub_140251E80(unsigned int tag_index, const char *save_path)`

Important behavior:

- Looks up the tag datum in `qword_14338D988`.
- Gets the tag group definition with `sub_14024E040(tag->group)`.
- Asserts the tag is not already in a forbidden save state.
- Calls backend wrapper `sub_140261ED0(group, save_path)` and stores the result in bit `0x02` of `tag->flags`.
  - In context, this acts like a read-only / cannot-save-over check.
- If read-only:
  - Logs either:
    - `tags: cannot save over the read-only tag '%s.%s'`
    - `tags: cannot save over the read-only tag '%s.%s' with '%s.%s'`
  - Returns failure.
- If backend alternate streams are enabled via `sub_140261FC0()`:
  - Gets the tag's stream list from `tag->stream_index` with `sub_1402C9520`.
  - For each stream descriptor, validates a required callback in the stream type table.
  - If a required stream callback fails, logs:
    - `tags: cannot save tag '%s.%s'; missing tag streams`
  - Returns failure.
- If the group has a virtual pre-save callback at `group->vtable_or_handler[+0x20/+0x28]`, calls it before serialization.
- Builds a compact stream save descriptor with `sub_1402C9150`.
- Calls `sub_1402624C0(group, save_path, tag_struct, tag_header_or_metadata, stream_descriptor)`.
  - IDA shows four parameters for `sub_1402624C0`, but the call site passes the stream descriptor too. Treat the decompiler prototype as incomplete.
- After save, if the tag is not marked with a high-bit flag, walks transient stream blocks and calls `tag_block_delete_elements` style cleanup through `sub_14024B360`.

## Backend Serialization Dispatch

`sub_1402624C0(...)`

This is a wrapper over the active tag backend table:

```c
backend = qword_14338DBF0;
return backend[0x60 / 8](...);
```

The active backend is installed during tag initialization. `sub_140262110()` initializes tag groups and calls `sub_1402622E0(off_141171418)` when a setup callback exists.

In the observed backend tables, `+0x60` resolves to the concrete writer wrapper:

- `off_14118B1E0` (`single_new`) has `+0x60 -> sub_1402C8B00`.
- `off_14118B300` (`monolithic_new`) has `+0x60 -> sub_1402C8B00`.

`sub_1402C8B00` validates `qword_1433A35D0` and its create/write backend at field `+24`, builds a save context, and calls `sub_1402C8910`.

`sub_1402C8910`:

- opens a writer through the create backend
- writes the main tag through `sub_1402C77B0`
- writes each alternate stream through the same persist function
- commits the writer on success
- aborts/discards the writer on failure

`sub_1402C77B0` creates `c_tag_group_files_persist_context` and calls `sub_14045EAE0`, the generic group-file persist walker. This is the actual format-sensitive serialization layer.

In the single-tag backend constructor `sub_1402CB3C0`, the backend structure includes:

- single-tag read backend
- synchronous read backend
- index backend
- attribute backend
- directory backend
- create backend
- xsync backend
- resource/cache-builder backend
- aggregate `s_tag_file_backend`

The single-file create backend vtable at `0x140D1A320` provides:

- `+0x08 -> sub_1402CB860` open a temporary writer
- `+0x10 -> sub_1402CCBF0` finalize and overwrite destination under `tags`
- `+0x18 -> sub_1402CB170` abort/discard temporary writer

## Save-As / Rename

`sub_140251D10(unsigned int tag_index, const char *new_path)`

Behavior:

- Rejects rename if another open tag already has the target path:
  - `tags: can't rename '%s' to '%s' because a tag with that name is already open.`
- Rejects rename when the current tag is using the tag file cache:
  - `tags: can't rename '%s.%s' because it is using the tag file cache.`
- Otherwise calls `sub_14024F590(tag_index, new_path)` and clears bits `0x06` from the tag flags.

## Reload Existing Open Tag

`sub_140251AC0(group, path, flags)` is not the save path, but it is important for live update/reload behavior.

Behavior:

- Finds an already-open tag by group/path with `sub_140247410`.
- If no open tag exists, loads it with `sub_14024FA50`.
- If an open tag exists and is reloadable:
  - Logs `REloading '%s.%s'` under `tags:reloading:`.
  - Saves a copy of the old 64-byte tag datum.
  - Calls `sub_14024FDC0(existing_tag_index, ..., group, path, flags)` to reload from disk/backend.
  - On success:
    - Calls `sub_140252E90(old_copy)` to dispose old blocks/resources.
    - Marks the tag datum with `word_flags |= 1`.
    - Notifies up to four registered observers in `qword_14116F6C8[]` via `sub_1400B55DC(observer, tag_index)`.
  - On failure:
    - Restores the old datum.
    - Logs `tags:reloading: failed to reload %s`.
