import { useEffect, useRef, type ReactNode } from 'react'
import { X } from 'lucide-react'

export default function Dialog({ title, children, onClose, locked = false }: { title: string; children: ReactNode; onClose: () => void; locked?: boolean }) {
  const ref = useRef<HTMLElement>(null)
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null
    (ref.current?.querySelector<HTMLElement>('input:not(:disabled), select:not(:disabled)') ?? ref.current?.querySelector<HTMLElement>('button:not(:disabled)'))?.focus()
    return () => previous?.focus()
  }, [])
  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === 'Escape' && !locked) { event.preventDefault(); onClose() }
    if (event.key !== 'Tab') return
    const focusable = [...(ref.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), summary, [tabindex="0"]') ?? [])]
    const first = focusable[0], last = focusable[focusable.length - 1]
    if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus() }
    else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus() }
  }
  return <div className="modal-backdrop" onMouseDown={e => { if (e.target === e.currentTarget && !locked) onClose() }}><section ref={ref} className="dialog" role="dialog" aria-modal="true" aria-labelledby="dialog-title" onKeyDown={onKeyDown}><div className="dialog-head"><h2 id="dialog-title">{title}</h2><button className="icon-button" aria-label="Close dialog" disabled={locked} onClick={onClose}><X size={18} /></button></div>{children}</section></div>
}
