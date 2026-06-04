package repo

import (
	"path/filepath"
	"testing"

	"github.com/touken928/wirehub/internal/config"
)

func TestCreateServiceMap_AllocatesVIPAndGroups(t *testing.T) {
	dir := t.TempDir()
	st, err := New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}
	settings := &Settings{
		WGSubnet: "100.127.0.0/24",
		HubIP:    "100.127.0.1",
		DNSIP:    "100.127.0.1",
	}
	if err := st.db.Create(settings).Error; err != nil {
		t.Fatal(err)
	}
	g, err := st.CreateGroup("map-users", 0, 0)
	if err != nil {
		t.Fatal(err)
	}

	detail, err := st.CreateServiceMap(MapInput{
		Slug:          "intranet",
		TargetHost:    "127.0.0.1",
		AllowedGroups: []uint{g.ID},
	})
	if err != nil {
		t.Fatal(err)
	}
	if detail.VirtualIP == "" || detail.VirtualIP == settings.HubIP {
		t.Fatalf("unexpected vip %q", detail.VirtualIP)
	}
	groups, err := st.ListMapGroupIDs(detail.ID)
	if err != nil || len(groups) != 1 || groups[0] != g.ID {
		t.Fatalf("groups = %v err=%v", groups, err)
	}
	ok, err := st.MapAllowedForPeer("100.127.0.2", detail.ID)
	if err != nil {
		t.Fatal(err)
	}
	if ok {
		t.Fatal("unknown peer should not be allowed")
	}
}
