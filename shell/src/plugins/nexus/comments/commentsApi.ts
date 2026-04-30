// Typed client for `com.nexus.comments` IPC handlers. The pane uses
// these wrappers exclusively so the View / store code never has to
// know about kernel handler ids or wire-decode details. Future
// editor-margin gutter (Phase 3) calls the same API to spawn
// threads.

import type { PluginAPI } from '../../../types/plugin'
import type { BlockId, Comment, CommentId, Thread, ThreadId } from './types'
import { decodeComment, decodeThread, decodeThreadList } from './decode'

const PLUGIN_ID = 'com.nexus.comments'

/** Bind every comment IPC handler against a `PluginAPI.kernel`. */
export interface CommentsApi {
  list(filePath: string): Promise<Thread[]>
  createThread(args: {
    filePath: string
    blockId: BlockId
    body: string
    author?: string
  }): Promise<Thread>
  addReply(args: {
    filePath: string
    threadId: ThreadId
    body: string
    author?: string
  }): Promise<Comment>
  setResolved(args: {
    filePath: string
    threadId: ThreadId
    resolved: boolean
    author?: string
  }): Promise<Thread>
  deleteThread(args: { filePath: string; threadId: ThreadId }): Promise<void>
  deleteComment(args: {
    filePath: string
    threadId: ThreadId
    commentId: CommentId
  }): Promise<void>
  editComment(args: {
    filePath: string
    threadId: ThreadId
    commentId: CommentId
    body: string
  }): Promise<Comment>
}

class DecodeError extends Error {
  constructor(handler: string) {
    super(`comments::${handler} returned a malformed payload`)
  }
}

export function createCommentsApi(kernel: PluginAPI['kernel']): CommentsApi {
  return {
    async list(filePath) {
      const raw = await kernel.invoke(PLUGIN_ID, 'list', {
        file_path: filePath,
      })
      return decodeThreadList(raw)
    },

    async createThread({ filePath, blockId, body, author }) {
      const raw = await kernel.invoke(PLUGIN_ID, 'create_thread', {
        file_path: filePath,
        block_id: blockId,
        body,
        ...(author !== undefined ? { author } : {}),
      })
      const t = decodeThread(raw)
      if (!t) throw new DecodeError('create_thread')
      return t
    },

    async addReply({ filePath, threadId, body, author }) {
      const raw = await kernel.invoke(PLUGIN_ID, 'add_reply', {
        file_path: filePath,
        thread_id: threadId,
        body,
        ...(author !== undefined ? { author } : {}),
      })
      const c = decodeComment(raw)
      if (!c) throw new DecodeError('add_reply')
      return c
    },

    async setResolved({ filePath, threadId, resolved, author }) {
      const raw = await kernel.invoke(PLUGIN_ID, 'set_resolved', {
        file_path: filePath,
        thread_id: threadId,
        resolved,
        ...(author !== undefined ? { author } : {}),
      })
      const t = decodeThread(raw)
      if (!t) throw new DecodeError('set_resolved')
      return t
    },

    async deleteThread({ filePath, threadId }) {
      await kernel.invoke(PLUGIN_ID, 'delete_thread', {
        file_path: filePath,
        thread_id: threadId,
      })
    },

    async deleteComment({ filePath, threadId, commentId }) {
      await kernel.invoke(PLUGIN_ID, 'delete_comment', {
        file_path: filePath,
        thread_id: threadId,
        comment_id: commentId,
      })
    },

    async editComment({ filePath, threadId, commentId, body }) {
      const raw = await kernel.invoke(PLUGIN_ID, 'edit_comment', {
        file_path: filePath,
        thread_id: threadId,
        comment_id: commentId,
        body,
      })
      const c = decodeComment(raw)
      if (!c) throw new DecodeError('edit_comment')
      return c
    },
  }
}
