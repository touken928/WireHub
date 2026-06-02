package wg

import (
	"sync"

	"golang.zx2c4.com/wireguard/tun"
)

// onceTUN ensures the underlying TUN device Close is only invoked once.
// wireguard-go's netstack TUN panics on double close.
type onceTUN struct {
	tun.Device
	once sync.Once
}

func (t *onceTUN) Close() error {
	var err error
	t.once.Do(func() {
		err = t.Device.Close()
	})
	return err
}
