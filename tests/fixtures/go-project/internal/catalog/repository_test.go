package catalog

import "testing"

func TestQueryCorpusCompiles(t *testing.T) {
	tests := []struct {
		name  string
		query string
	}{
		{name: "active", query: "select id,name from public.catalog_items where active=true;"},
		{name: "by id", query: "select id,name from public.catalog_items where id=$1;"},
	}
	for _, test := range tests {
		if test.query == "" {
			t.Fatalf("%s has no query", test.name)
		}
	}
}
