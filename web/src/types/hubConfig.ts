export function upstreamDnsToText(servers?: string[]): string {
  return (servers ?? []).join('\n');
}

export function textToUpstreamDns(text: string): string[] {
  return text.split('\n').map((line) => line.trim()).filter(Boolean);
}
