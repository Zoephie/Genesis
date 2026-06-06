# Sapien Update Notes

## What Was Searched

I searched names/strings/xrefs for:

- `sapien`
- `SendMessage`
- `PostMessage`
- `FindWindow`
- `RegisterWindowMessage`
- `WM_COPYDATA`
- `reload`
- `refresh`
- `notify`
- `xsync`
- `pending sync`

## What Was Found

Only one `FindWindowA` caller was found:

- `sub_14033EFE0`

This function searches windows, compares window titles including `H3 Sapien`, captures a window bitmap/DC, and logs screenshot-like errors:

- `could not create a bitmap handle for the window`
- `could not write window to bitmap`
- `could not get the device context for the window`

This is not part of the tag save path.

The save path itself does not call `SendMessageA`, `PostMessageA`, `FindWindowA`, or a visible `WM_COPYDATA` style notification.

## What Likely Updates Sapien

The strongest evidence points to shared backend reload behavior:

1. Guerilla writes the tag through the active tag backend.
2. Sapien already has the tag open or cached.
3. Sapien's tag system reloads changed/pending-sync tags through the normal reload path.

Important reload function:

`sub_140251AC0(group, path, flags)`

This function:

- Finds an already-open tag by group/path.
- Reloads it using `sub_14024FDC0`.
- Replaces the in-memory datum on success.
- Disposes the old blocks/resources.
- Notifies registered in-process tag observers in `qword_14116F6C8[]`.

Important xsync/pending-sync function:

`sub_1404698F0(monolithic_backend)`

This function:

- Builds/checks `<backend root>/xsync_write_lock`.
- Builds/checks another path using `a1 + 280`.
- Logs `reloading tag cache from pending sync...`.
- Logs `tags:monolithic: reloading from sync`.
- Compares an index file timestamp/hash/state against `a1 + 865`.
- Logs `tags:monolithic: index file has not been updated for current sync` if the pending sync does not match.

Important single-file xsync difference callback:

`sub_140269080(state, event_type, changed_path)`

This function:

- Handles changed-path event type `3`.
- Parses the changed path into tag group/path with `sub_140261D70`.
- Finds an already-known/open tag index with `sub_140250D30`.
- Reads current backend checksum/state with `sub_140261C30`.
- Compares that with the loaded tag checksum from `sub_14024CAD0(tag_index)`.
- Compares old/new read-only state.
- Logs `tags:sync:pc: ... RELOADING!` when reload is required.
- Marks a bit for the changed tag index and sets a pending flag byte.

That is likely the "real time" part for single-file backend mode: xsync notices that a backend path changed and marks the loaded tag for reload. Guerilla's save path does not call this directly; it writes backend artifacts that this scanner can detect.

## Backport Implication

For a custom tool, the safest interpretation is:

- Do not try to send a window message to Sapien first; there is no evidence Guerilla does this for save.
- Write the tag exactly as Guerilla's backend writer expects.
- Preserve sidecar/alternate stream behavior when enabled.
- If Sapien is running in a backend mode that expects xsync/pending-sync state, update the same sync/index/lock artifacts, or Sapien may continue using stale cached data.
- If Sapien is simply watching tag file timestamps or reloading through the shared backend, an exact atomic overwrite of the tag file may be enough.
- For Rust specifically: feasible if the Rust tool implements the exact backend writer format and commit semantics. The hard part is the group-file persist layout and alternate stream/resource data, not the language.

## Unresolved

Still unresolved:

- Full struct layouts for tag datum, stream iterator, `s_tag_file_backend`, and `c_tag_group_files_persist_context`.
- Exact binary format details inside `sub_14045EAE0` and the generic group-file persist operation it calls.
- Whether Sapien in the user's specific run is using `single_new`, `monolithic_new`, or another backend mode.
- The exact external artifact set needed to wake Sapien in every backend mode.
