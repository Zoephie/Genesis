# Backport Checklist

Use this as the implementation checklist for a custom tag-saving tool.

## Required Save Semantics

- Resolve the tag's group and path exactly the same way Guerilla does.
- Reject saving over read-only backend paths.
- Reject save-as if another open/known tag has the target name.
- Reject save-as if the tag is using the tag file cache.
- Preserve alternate tag streams when backend streams are enabled.
- Run or replicate group-specific pre-save fixup/validation before serialization.
- Serialize through the same layout rules as the tag backend writer.
- Use an atomic replace/overwrite strategy for the final tag file.
- Clear or update dirty/version/timestamp metadata after a successful write.

## Alternate Streams

`sub_140251E80` checks backend stream support with `sub_140261FC0()`.

When enabled:

- It obtains stream descriptors from the tag datum's stream index.
- It resolves each stream type with `sub_140252340`.
- If a stream type has the required flag set, it calls that stream type's validation/write callback.
- Save fails with `missing tag streams` if a required callback fails.

Backport note: if your tool ignores these streams, Sapien may load stale or incomplete companion data even if the main tag file writes successfully.

## Post-Save Update

After a successful save, Guerilla:

- Calls `sub_1400DE720(tag_index)` to update cached backend file metadata/path state.
- Removes the tag from the dirty set with `sub_1400DE600(tag_index, 0)`.
- Notifies in-process observers with `sub_1400DD1A0(tag_index, 3)`.

Backport note: your external tool cannot call Guerilla's in-process observers. For Sapien, the relevant replacement is probably file/backend-level consistency: timestamps, sync state, and sidecar streams.

## Sapien Live Update Strategy

Recommended order:

1. First identify the backend mode Sapien is using (`single_new`, `monolithic_new`, or another table).
2. Implement exact tag serialization and atomic overwrite for that backend.
3. Preserve alternate streams/data chunks; both observed backend tables report stream support enabled.
4. Test with Sapien open on the tag and observe whether Sapien reloads automatically.
5. If Sapien does not update, inspect/update xsync artifacts:
   - `xsync_write_lock`
   - pending sync path under the backend root
   - monolithic index file state checked by `sub_1404698F0`
   - single-file xsync changed-path/checksum state observed in `sub_140269080`
6. If still stale, trigger Sapien's own reload command/path rather than inventing a Guerilla-to-Sapien message.

## Rust Program Feasibility

Feasible, with a medium-to-high reverse-engineering cost.

Rust is a good fit for a deterministic backend writer once the format is known. The risky parts are:

- reproducing `sub_14045EAE0` group-file persistence exactly
- applying group-specific pre-save fixups before serialization
- writing alternate streams and resources, not just the main tag body
- finalizing header/checksum fields the way `c_single_tag_file_writer` does
- using temporary-write then final-overwrite semantics
- making changes visible to xsync/reload logic

For a first milestone, implement one low-risk tag group with no required extra resources, then compare bytes/checksums against Guerilla's output for the same edit.

## Do Not Assume

- Do not assume a Win32 message is involved. I did not find one in the save path.
- Do not assume main `.tag` file bytes are the whole save. Required streams and resource/cache data can matter.
- Do not assume Save and Reload are the same function. Save uses `sub_140251E80`; reload uses `sub_140251AC0` and `sub_14024FDC0`.

## Suggested Names To Apply In IDA

These names are descriptive, not recovered official symbols:

- `sub_1400DE560` -> `guerilla_save_tag_or_save_as`
- `sub_140251E50` -> `tag_save_current_path`
- `sub_140251E80` -> `tag_save_core`
- `sub_140251D10` -> `tag_rename_for_save_as`
- `sub_1402624C0` -> `tag_backend_save_dispatch`
- `sub_1402C8B00` -> `backend_save_tag_to_create_backend`
- `sub_1402C8910` -> `write_tag_and_streams_to_create_backend`
- `sub_1402C77B0` -> `persist_tag_group_data_to_stream`
- `sub_14045EAE0` -> `tag_group_files_persist_walk`
- `sub_140251AC0` -> `tag_reload_existing_or_load`
- `sub_14024FDC0` -> `tag_load_core`
- `sub_1404698F0` -> `monolithic_reload_from_pending_xsync`
- `sub_140269080` -> `single_tag_xsync_difference_callback`
