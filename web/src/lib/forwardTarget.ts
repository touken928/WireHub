/** Client-side check aligned with domain.ValidateForwardTargetHost. */
export function validateForwardTargetHost(host: string): string | null {
  const h = host.trim().toLowerCase().replace(/\.$/, '');
  if (!h) {
    return 'Target host is required';
  }
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(h)) {
    const parts = h.split('.').map(Number);
    if (parts.some((p) => p > 255)) {
      return 'Invalid IPv4 address';
    }
    return null;
  }
  if (!h.includes('.')) {
    return 'Use a hostname (e.g. peer.wirehub) or IPv4 address, not a peer username';
  }
  if (h.length > 253) {
    return 'Hostname is too long';
  }
  return null;
}
