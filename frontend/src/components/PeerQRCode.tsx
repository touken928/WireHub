import { Component, type ReactNode } from 'react'
import { QRCodeSVG } from 'qrcode.react'
import { QrCode } from 'lucide-react'

// Large configurations can exceed QR capacity. Keep file/copy import available.
class QRBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false }
  static getDerivedStateFromError() { return { failed: true } }
  render() {
    return this.state.failed
      ? <div className="qr-fallback" role="status"><QrCode size={28} /><p>This configuration is too large for a QR code. Download or copy it to import instead.</p></div>
      : this.props.children
  }
}

export default function PeerQRCode({ config }: { config: string }) {
  return <QRBoundary key={config}><QRCodeSVG value={config} size={240} level="M" marginSize={4} role="img" aria-label="WireGuard configuration QR code" /></QRBoundary>
}
