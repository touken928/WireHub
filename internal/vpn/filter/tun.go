package filter

import (
	"net/netip"
	"os"
	"sync"

	"golang.zx2c4.com/wireguard/tun"
)

// TUN wraps a device and enforces per-peer group ACLs at the IP layer.
type TUN struct {
	inner     tun.Device
	hubIP     netip.Addr
	mu        sync.RWMutex
	closeOnce sync.Once
	access    *RuleSet
}

func NewTUN(inner tun.Device, hubIP netip.Addr) *TUN {
	return &TUN{
		inner:  inner,
		hubIP:  hubIP,
		access: NewRuleSet(),
	}
}

func (f *TUN) SetAccessRules(rules *RuleSet) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if rules == nil {
		f.access = NewRuleSet()
		return
	}
	f.access = rules
}

func (f *TUN) shouldDrop(packet []byte) bool {
	if len(packet) < 20 {
		return false
	}
	version := packet[0] >> 4
	if version != 4 {
		return false
	}

	ihl := int(packet[0]&0x0f) * 4
	if len(packet) < ihl {
		return false
	}

	src := netip.AddrFrom4([4]byte{packet[12], packet[13], packet[14], packet[15]})
	dst := netip.AddrFrom4([4]byte{packet[16], packet[17], packet[18], packet[19]})

	if src == f.hubIP || dst == f.hubIP {
		return false
	}

	f.mu.RLock()
	access := f.access
	f.mu.RUnlock()

	if !access.CanAccess(src, dst) || !access.CanAccess(dst, src) {
		return true
	}
	return false
}

func (f *TUN) Name() (string, error)       { return f.inner.Name() }
func (f *TUN) File() *os.File              { return f.inner.File() }
func (f *TUN) Events() <-chan tun.Event    { return f.inner.Events() }
func (f *TUN) MTU() (int, error)           { return f.inner.MTU() }
func (f *TUN) BatchSize() int              { return f.inner.BatchSize() }

func (f *TUN) Read(bufs [][]byte, sizes []int, offset int) (int, error) {
	return f.inner.Read(bufs, sizes, offset)
}

func (f *TUN) Write(bufs [][]byte, offset int) (int, error) {
	filtered := make([][]byte, 0, len(bufs))
	for _, b := range bufs {
		packet := b[offset:]
		if f.shouldDrop(packet) {
			continue
		}
		filtered = append(filtered, b)
	}
	if len(filtered) == 0 {
		return len(bufs), nil
	}
	return f.inner.Write(filtered, offset)
}

func (f *TUN) Close() error {
	var err error
	f.closeOnce.Do(func() {
		err = f.inner.Close()
	})
	return err
}
