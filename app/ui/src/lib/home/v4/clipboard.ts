/** Clipboard is the only output: no file, upload, or text fallback. */
export async function copyPng(blob: Blob): Promise<'copied' | 'failed'> {
  try {
    if (!navigator.clipboard?.write || typeof ClipboardItem === 'undefined') return 'failed';
    await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })]);
    return 'copied';
  } catch { return 'failed'; }
}
