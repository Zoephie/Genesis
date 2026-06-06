# Raw Evidence

Selected decompiler evidence, shortened for readability.

## `sub_1400DE560` Save Wrapper

```c
if (tag_index == -1)
    return 0;

if (new_path && strcmp_like(new_path, current_path))
{
    if (!sub_140251D10(tag_index, new_path))
        return 0;
    sub_1400DE720(tag_index);
}

sub_1400DD1A0(tag_index, 2);
ok = sub_140251E50(tag_index);
if (ok)
{
    sub_1400DE720(tag_index);
    sub_1400DE600(tag_index, 0);
    sub_1400DD1A0(tag_index, 3);
}
return ok;
```

## `sub_140251E50` Current-Path Save

```c
tag = sub_14030CD50(qword_14338D988, tag_index);
return sub_140251E80(tag_index, tag->path);
```

## `sub_140251E80` Save Core Evidence

Strings:

- `tags: cannot save tag '%s.%s'; missing tag streams`
- `tags: cannot save over the read-only tag '%s.%s' with '%s.%s'`
- `tags: cannot save over the read-only tag '%s.%s'`

Important calls:

- `sub_14030CD50(qword_14338D988, tag_index)` - get tag datum.
- `sub_14024E040(tag->group)` - get group definition.
- `sub_140261ED0(group, save_path)` - backend read-only/cannot-overwrite check.
- `sub_140261FC0()` - alternate stream support.
- `sub_1402C9520(tag->stream_index, ...)` - get stream descriptors.
- `group_callback(tag_struct, stream_payload)` - group-specific pre-save callback.
- `sub_1402C9150(...)` - compact stream descriptor.
- `sub_1402624C0(group, save_path, tag_struct, metadata, stream_descriptor)` - backend save dispatch.
- `sub_14024B360(...)` - cleanup transient tag block data after save.

## `sub_1402624C0` Backend Save Dispatch

```c
if (!byte_14338DBFC)
    assert();

backend = qword_14338DBF0;
if (!backend)
    assert();

return backend_table_entry_at_plus_0x60(...);
```

Observed table resolution:

```text
off_14118B1E0 ("single_new")    +0x60 -> sub_1402C8B00
off_14118B300 ("monolithic_new") +0x60 -> sub_1402C8B00
```

## `sub_1402C8B00` Backend Save Wrapper

Important behavior:

```c
backend_context = qword_1433A35D0;
if (!backend_context)
    assert();

create_backend = *(backend_context + 24);
if (!create_backend)
    return 0;

build c_tag_group_files_allocator;
build c_tag_field_editor_manager_default;
build save context;

return sub_1402C8910(context, create_backend, group, path, 0,
                     tag_struct, metadata, stream_iterator);
```

## `sub_1402C8910` Main Writer

Important behavior:

```c
writer = create_backend->open(group, path, flags);
main_output = writer->open_main_stream();
ok = sub_1402C77B0(main_output, context, tag_struct);

while (ok && stream_iterator && stream_iterator->next())
{
    stream_id = stream_iterator->stream_id();
    stream_data = stream_iterator->stream_data();
    stream_output = writer->open_named_stream(stream_id, stream_name);
    ok = sub_1402C77B0(stream_output, context, stream_data);
    ok = writer->commit_current_stream(stream_output) && ok;
}

if (ok)
    return create_backend->commit(writer, path, metadata);

create_backend->abort(writer);
return 0;
```

## `sub_14045EAE0` Persist Walker

Important behavior:

```c
output = persist_context->get_output();
allocator_or_file_context = persist_context->get_allocator_context();
group_name = qword_1433A0938 + 4 * *(tag_or_stream_data + 8);

operation = allocator_or_file_context->create_operation(41, group_name);
return operation->persist(0, tag_or_stream_data, group_name,
                          layout_context, persist_context, -2);
```

## Single-File Commit Evidence

`c_single_tag_file_create_backend::vftable` at `0x140D1A320`:

```text
+0x08 -> sub_1402CB860 open temp writer
+0x10 -> sub_1402CCBF0 commit final file
+0x18 -> sub_1402CB170 abort/discard
```

Commit strings:

- `failed to write out tag header`
- `failed to write out final tag file header`
- `tags:tag_save: couldn't overwrite '%s.%s' with '%s'`
- `failed to commit data chunk`

`c_single_tag_file_writer::vftable` at `0x140DBEBC8`:

```text
+0x00 -> sub_140471B20 open main stream
+0x08 -> sub_1404718D0 open named stream
+0x10 -> sub_1404716E0 commit current chunk
```

## `sub_140251D10` Save-As/Rename Evidence

Strings:

- `tags: can't rename '%s' to '%s' because a tag with that name is already open.`
- `tags: can't rename '%s.%s' because it is using the tag file cache.`

Important behavior:

```c
if (find_open_tag_same_group_and_path(group, new_path) != -1)
    fail;

if ((tag->flags & 5) || !backend_uses_tag_file_cache(group, tag->path))
{
    sub_14024F590(tag_index, new_path);
    tag->flags &= 0xF9;
    return 1;
}

fail;
```

## `sub_140251AC0` Reload Existing Tag

Strings:

- `REloading '%s.%s'`
- `tags:reloading:`
- `tags:reloading: failed to reload %s`

Important behavior:

```c
existing = find_open_tag(group, path);
if (existing == -1)
    return load_tag(group, path, flags);

old_datum = *tag_datum(existing);
ok = sub_14024FDC0(existing, ..., group, path, flags);
tag->flags |= 4;

if (ok)
{
    dispose_old_datum(&old_datum);
    tag->word_flags |= 1;
    for each observer in qword_14116F6C8[0..3]:
        sub_1400B55DC(observer, existing);
}
else
{
    dispose_new_failed_datum(tag);
    *tag = old_datum;
    log failed reload;
}
```

## `sub_1404698F0` Pending Xsync Reload

Strings:

- `xsync_write_lock`
- `reloading tag cache from pending sync...`
- `tags:monolithic: reloading from sync`
- `tags:monolithic: index file has not been updated for current sync`

Important behavior:

```c
path = backend_root + "xsync_write_lock";
if (exists(path))
    return 0;

pending = backend_root + *(a1 + 280);
if (!exists(pending) || !read_file_time_or_state(pending))
    return 0;

print("reloading tag cache from pending sync...");
log("tags:monolithic: reloading from sync");

if (read_pending_state(pending, &state))
{
    if (!matches_current_index(a1 + 865, state))
        log("index file has not been updated for current sync");
}

return 1;
```

## `sub_140269080` Single-File Xsync Difference Callback

String:

- `tags:sync:pc: %s, old checksum 0x%08x, new checksum 0x%08x, was read only: %s, now read only: %s, %s`

Important behavior:

```c
if (event_type == 3)
{
    changed_path = parse_changed_path(a3);
    if (sub_140261D70(changed_path, &group, path))
    {
        tag_index = sub_140250D30(group, path);
        if (tag_index != -1)
        {
            old_readonly = sub_1402515C0(tag_index);
            new_readonly = sub_140261ED0(group, path);
            has_new_state = sub_140261C30(group, path, &new_checksum);
            should_reload = has_new_state &&
                (old_readonly != new_readonly ||
                 new_checksum != sub_14024CAD0(tag_index));

            if (should_reload)
            {
                mark_bit_for_tag_index(tag_index);
                state[0] = 1;
            }
        }
    }
}
```

## Negative Evidence

Searches did not find a save-path direct message to Sapien:

- `SendMessageA` import has many callers, but none in `sub_1400DE560`, `sub_140251E80`, `sub_1402624C0`, or the adjacent tag save/reload cluster.
- `PostMessageA` import has callers, but none in the save path.
- `FindWindowA` only led to `sub_14033EFE0`, a window capture helper that references `H3 Sapien`.
- No `WM_COPYDATA` or `RegisterWindowMessage` name/string was found.
