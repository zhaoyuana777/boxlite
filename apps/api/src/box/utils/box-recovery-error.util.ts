import { sanitizeBoxError } from './sanitize-error.util'

// Classify evidence, but never publish arbitrary runner messages (paths or credentials).
export function boxRecoveryError(error: unknown): string {
  const reason = sanitizeBoxError(error).errorReason ?? ''
  let guidance: string
  if (/no space left|ENOSPC|quota exceeded|EDQUOT/i.test(reason)) {
    guidance =
      'Storage space or quota is exhausted. Ask an administrator to check host capacity and the guest filesystem; free space or expand the affected storage before retrying.'
  } else if (/not found|no such file|missing.*disk|disk.*missing/i.test(reason)) {
    guidance =
      'The original box or a required disk file is missing. Ask an administrator to locate or restore the original files from backup; otherwise explicitly rebuild the box.'
  } else if (/permission denied|EACCES|read-only file system|EROFS/i.test(reason)) {
    guidance =
      'Storage access was denied or the filesystem is read-only. Ask an administrator to check permissions and mount health before retrying.'
  } else if (/corrupt|invalid qcow|invalid.*disk header/i.test(reason)) {
    guidance =
      'The runtime reported disk corruption. Preserve a copy of the disk and ask an administrator to repair it offline or restore a backup before retrying.'
  } else if (/timed out|timeout|deadline exceeded/i.test(reason)) {
    guidance = 'The operation timed out. Check runner availability and VM status before retrying.'
  } else if (/auto-delete/i.test(reason)) {
    guidance =
      'The original box is configured for automatic deletion on stop. Recovery was refused to protect its disk; ask an administrator to preserve the data.'
  } else if (/stop original VM/i.test(reason)) {
    guidance = 'The original VM could not be stopped. Check its process and disk lock before retrying.'
  } else if (/volume mount/i.test(reason)) {
    guidance =
      'An original volume could not be mounted. Check storage connectivity and mount permissions before retrying.'
  } else {
    guidance =
      'The original VM could not be confirmed running. Check runner logs for the cause, fix it and retry; explicitly rebuild only if the original box cannot be repaired.'
  }
  return `Box recovery failed. ${guidance} Recovery does not delete or replace the original box.`
}
