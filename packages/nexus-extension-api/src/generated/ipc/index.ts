// Auto-generated barrel for the Phase 4 WI-36 pilot IPC handler types.
//
// The individual `.ts` siblings of this file are produced by
// `cargo test -p nexus-storage --features ts-export` and
// `cargo test -p nexus-ai --features ts-export` and must NOT be edited
// by hand. This barrel itself is hand-authored — keep it in sync when
// the pilot grows in v1.1 (see `docs/ipc-schemas.md`).
//
// The 5 pilot handlers (Phase 4 §3.1):
//   com.nexus.storage::search      — StorageSearch*
//   com.nexus.storage::read_file   — StorageReadFile*
//   com.nexus.storage::write_file  — StorageWriteFile*
//   com.nexus.storage::list_dir    — StorageListDir*
//   com.nexus.ai::stream_ask       — AiStreamAsk*

// com.nexus.storage::search
export type { StorageSearchArgs } from './StorageSearchArgs';
export type { StorageSearchHit } from './StorageSearchHit';
export type { StorageSearchResult } from './StorageSearchResult';

// com.nexus.storage::read_file
export type { StorageReadFileArgs } from './StorageReadFileArgs';
export type { StorageReadFileResult } from './StorageReadFileResult';

// com.nexus.storage::write_file
export type { StorageWriteFileArgs } from './StorageWriteFileArgs';
export type { StorageWriteFileResult } from './StorageWriteFileResult';

// com.nexus.storage::list_dir
export type { StorageListDirArgs } from './StorageListDirArgs';
export type { StorageListDirEntry } from './StorageListDirEntry';
export type { StorageListDirResult } from './StorageListDirResult';

// com.nexus.ai::stream_ask
export type { AiStreamAskArgs } from './AiStreamAskArgs';
export type { AiStreamAskMessage } from './AiStreamAskMessage';
export type { AiStreamAskRole } from './AiStreamAskRole';
export type { AiStreamAskResult } from './AiStreamAskResult';
export type { AiStreamAskSource } from './AiStreamAskSource';

// com.nexus.ai::stream_chat (BL-010 / BL-011 / BL-034 — shared chat/complete engine)
export type { AiStreamChatArgs } from './AiStreamChatArgs';
export type { AiStreamChatMode } from './AiStreamChatMode';
export type { AiToolPolicy } from './AiToolPolicy';
