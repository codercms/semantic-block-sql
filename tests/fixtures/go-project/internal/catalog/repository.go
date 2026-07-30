package catalog

import "context"

type Queryer interface {
	QueryContext(context.Context, string, ...any) (any, error)
	ExecContext(context.Context, string, ...any) (any, error)
}

type Repository struct {
	DB Queryer
}

type Queries struct {
	FindActive string
	Archive    string
}

func wrap(query string) string { return query }

func (repository Repository) FindActive(ctx context.Context, minimumID int64) (any, error) {
	return repository.DB.QueryContext(ctx, "select id,name,metadata from public.catalog_items where active=true and id>=$1 order by id;", minimumID)
}

func (repository Repository) Archive(ctx context.Context, id int64) (any, error) {
	return repository.DB.ExecContext(ctx, wrap("update public.catalog_items set archived_at=now() where id=$1 returning id;"), id)
}

func DefaultQueries() Queries {
	return Queries{
		FindActive: "select id,name from public.catalog_items where active=true order by name;",
		Archive:    "update public.catalog_items set archived_at=now() where id=$1;",
	}
}

func StaticQuery() string {
	return "SELECT " + "id,name " + "FROM public.catalog_items WHERE active=true " + "AND id>0;"
}

func DynamicQuery(columns string) string {
	return "SELECT " + columns + " FROM public.catalog_items"
}

const markerQuery = "select '`' as marker,id from public.catalog_items where id=$1;"

func MarkerQuery() string { return markerQuery }
