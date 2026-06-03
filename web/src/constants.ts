/** Default DNS suffix when hub settings are not loaded yet. */
export const DNS_DOMAIN = 'wirehub';

/** Hub hostname label (hub.wirehub). */
export const HUB_DNS_LABEL = 'hub';

export function hubFQDN(suffix = DNS_DOMAIN) {
  return `${HUB_DNS_LABEL}.${suffix}`;
}

export const API_BASE = '/api';

export const TOKEN_STORAGE_KEY = 'wirehub_token';

export const THEME_STORAGE_KEY = 'wirehub_theme';
