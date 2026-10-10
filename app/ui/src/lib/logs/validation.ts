/** Match the UTF-8 byte and control-character limits in ChatSettings::valid. */
export function validHiddenTerm(value: string, maxBytes: number): boolean {
  const bytes = new TextEncoder().encode(value).length;
  return bytes >= 1 && bytes <= maxBytes && !/\p{Cc}/u.test(value);
}
