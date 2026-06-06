# Guerilla Tag Save Findings

Reverse-engineering notes from the IDA database currently served on `127.0.0.1:13370`.

Goal: identify what Guerilla does when saving a tag, and what evidence exists for why Sapien sees the updated tag.

## Short Conclusion

Guerilla's normal "save current tag" path is:

1. UI/editor code calls `sub_1400DE560(tag_index, optional_new_path)`.
2. `sub_1400DE560` optionally renames the tag, marks the tag as saving, calls `sub_140251E50(tag_index)`, then clears dirty/save state on success.
3. `sub_140251E50` loads the tag's current path from the tag datum and calls `sub_140251E80(tag_index, path)`.
4. `sub_140251E80` performs the real save checks and dispatches serialization through the active tag backend via `sub_1402624C0(...)`.
5. `sub_1402624C0` calls the active backend table entry at offset `+0x60`.
6. In both observed `single_new` and `monolithic_new` backend tables, `+0x60` resolves to `sub_1402C8B00`, which opens the active create/write backend, writes the main tag plus alternate streams, and commits the finished backend artifact.

I did not find a direct Guerilla-to-Sapien Win32 notification in the save path. Searches for `SendMessageA`, `PostMessageA`, `FindWindowA`, `WM_COPYDATA`, and `sapien` did not produce a save-related broadcast. The one `FindWindowA` path found is a window screenshot helper, not tag save/update.

The likely Sapien update mechanism is backend/file based: Guerilla writes the tag through the shared tag backend; Sapien has tag reload/xsync logic that reloads changed/pending-sync tag data. Evidence for the reload side is in `sub_140251AC0` and `sub_1404698F0`, with strings such as `tags:reloading:`, `tags:reloading: failed to reload %s`, and `reloading tag cache from pending sync...`.

## Files

- `save-flow.md` - call chain and behavior of save/save-as.
- `function-map.md` - important functions and data addresses.
- `backend-writer.md` - deeper notes on backend table slots, create backend commit, stream writer, persist walker, and xsync difference detection.
- `sapien-update-notes.md` - what was found, what was not found, and how Sapien probably sees changes.
- `backport-checklist.md` - practical implementation checklist for a custom tool.
- `raw-evidence.md` - selected decompiler evidence and strings.

## Confidence

High confidence:

- `sub_1400DE560 -> sub_140251E50 -> sub_140251E80 -> sub_1402624C0` is the main save path.
- `sub_140251E80` handles read-only checks, missing stream checks, group pre-save callback, backend serialization, and cleanup of transient tag blocks.
- No direct save-path `SendMessageA`/`PostMessageA`/`FindWindowA` notification was found in this pass.
- Backend table `+0x60` is `sub_1402C8B00` for both `single_new` and `monolithic_new` tables seen in this database.
- Single-file create backend commit overwrites under the `tags` root after writing a temporary tag file.

Medium confidence:

- Sapien update is driven by the shared tag file backend plus reload/xsync machinery rather than a direct message from Guerilla.
- To backport live update, faithfully writing the same backend artifacts is necessary but may not be sufficient; xsync/difference state may be needed depending on the tag backend mode Sapien is using.
