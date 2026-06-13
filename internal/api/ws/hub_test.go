package ws

import (
	"net/http"
	"net/url"
	"testing"
)

func TestOriginMatchesHost_ExactMatch(t *testing.T) {
	if !originMatchesHost("https://example.com", "example.com") {
		t.Fatal("expected https://example.com to match host example.com")
	}
	if !originMatchesHost("http://example.com", "example.com") {
		t.Fatal("expected http://example.com to match host example.com")
	}
}

func TestOriginMatchesHost_WithPort(t *testing.T) {
	if !originMatchesHost("https://example.com:8443", "example.com:8443") {
		t.Fatal("expected https://example.com:8443 to match host example.com:8443")
	}
}

func TestOriginMatchesHost_Mismatch(t *testing.T) {
	if originMatchesHost("https://attacker.com", "example.com") {
		t.Fatal("expected https://attacker.com to NOT match host example.com")
	}
}

func TestOriginMatchesHost_NoScheme(t *testing.T) {
	if originMatchesHost("example.com", "example.com") {
		t.Fatal("expected origin without scheme to NOT match")
	}
}

func TestOriginMatchesHost_EmptyOrigin(t *testing.T) {
	if originMatchesHost("", "example.com") {
		t.Fatal("expected empty origin to NOT match")
	}
}

func TestCheckOrigin_NoOrigin(t *testing.T) {
	req := &http.Request{
		Header: http.Header{},
		Host:   "example.com",
	}
	if !upgrader.CheckOrigin(req) {
		t.Fatal("expected request without Origin header to be allowed")
	}
}

func TestCheckOrigin_SameOrigin(t *testing.T) {
	tests := []struct {
		name   string
		origin string
		host   string
	}{
		{"same host exact", "https://example.com", "example.com"},
		{"same host with port", "http://localhost:8443", "localhost:8443"},
		{"https same", "https://example.com:8443", "example.com:8443"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			req := &http.Request{
				Header: http.Header{"Origin": []string{tc.origin}},
				Host:   tc.host,
			}
			if !upgrader.CheckOrigin(req) {
				t.Errorf("expected origin %q to be allowed for host %q", tc.origin, tc.host)
			}
		})
	}
}

func TestCheckOrigin_CrossOrigin(t *testing.T) {
	req := &http.Request{
		Header: http.Header{"Origin": []string{"https://attacker.com"}},
		Host:   "example.com",
	}
	if upgrader.CheckOrigin(req) {
		t.Fatal("expected cross-origin request to be rejected")
	}
}

func TestCheckOrigin_NoSchemeOrigin(t *testing.T) {
	req := &http.Request{
		Header: http.Header{"Origin": []string{"evil.com"}},
		Host:   "example.com",
	}
	if upgrader.CheckOrigin(req) {
		t.Fatal("expected origin without scheme to be rejected")
	}
}

// TestRequestRoundTrip constructs a realistic request similar to
// what a browser or test client would send.
func TestCheckOrigin_RealisticSameOrigin(t *testing.T) {
	req := &http.Request{
		Method: http.MethodGet,
		URL:    &url.URL{Scheme: "http", Host: "localhost:8080", Path: "/api/ws/status"},
		Header: http.Header{
			"Origin":       []string{"http://localhost:8080"},
			"Connection":   []string{"Upgrade"},
			"Upgrade":      []string{"websocket"},
			"Sec-WebSocket-Key":    []string{"dGhlIHNhbXBsZSBub25jZQ=="},
			"Sec-WebSocket-Version": []string{"13"},
		},
		Host: "localhost:8080",
	}
	if !upgrader.CheckOrigin(req) {
		t.Fatal("expected same-origin localhost request to be allowed")
	}
}

func TestCheckOrigin_RealisticCrossOrigin(t *testing.T) {
	req := &http.Request{
		Method: http.MethodGet,
		URL:    &url.URL{Scheme: "http", Host: "localhost:8080", Path: "/api/ws/status"},
		Header: http.Header{
			"Origin":       []string{"http://evil.example.com"},
			"Connection":   []string{"Upgrade"},
			"Upgrade":      []string{"websocket"},
			"Sec-WebSocket-Key":    []string{"dGhlIHNhbXBsZSBub25jZQ=="},
			"Sec-WebSocket-Version": []string{"13"},
		},
		Host: "localhost:8080",
	}
	if upgrader.CheckOrigin(req) {
		t.Fatal("expected cross-origin request to be rejected")
	}
}
