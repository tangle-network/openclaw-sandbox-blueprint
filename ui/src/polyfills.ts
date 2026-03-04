// Ensure crypto.randomUUID is available on non-secure local HTTP contexts.
// Some runtime/session libs rely on it for client-side IDs.
(function ensureRandomUuid() {
  const g = globalThis as typeof globalThis & {
    crypto?: Crypto;
  };

  if (!g.crypto) {
    return;
  }
  if (typeof g.crypto.randomUUID === 'function') {
    return;
  }
  if (typeof g.crypto.getRandomValues !== 'function') {
    return;
  }

  g.crypto.randomUUID = function randomUUID(): `${string}-${string}-${string}-${string}-${string}` {
    const bytes = new Uint8Array(16);
    g.crypto!.getRandomValues(bytes);

    // RFC 4122 version 4 + variant bits.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    const hex = [...bytes].map((value) => value.toString(16).padStart(2, '0')).join('');
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}` as `${string}-${string}-${string}-${string}-${string}`;
  };
})();
