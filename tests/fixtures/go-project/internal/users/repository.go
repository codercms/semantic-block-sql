package users

import (
	"context"
	"database/sql"
)

const findByID = `
    select id,name,email from public.users where id=$1;
`

const (
	insertUser = `insert into public.users (name,email) values ($1,$2) returning id;`
	deleteUser = `delete from public.users where id=$1 returning id;`
)

var listActive = `
    select id,name,email from public.users where active=true order by name;
`

var prepared = mustPrepare(`
    select id,name from public.users where active=true;
`)

type Repository struct {
	DB *sql.DB
}

func (repository Repository) Load(ctx context.Context, id int64) (*sql.Rows, error) {
	const lookup = `
        select id,name,email from public.users where id=$1;
	`

	query := `select id,name from public.users order by id;`
	query = `select id,name from public.users where active=true order by id;`

	rows, err := repository.DB.QueryContext(ctx, `
        select id,name,email from public.users where active=true and id>=$1 order by id;
	`, id)
	_ = query
	_ = prepared
	return rows, err
}

func (repository Repository) ReturnActive(ctx context.Context, minimumID int64) (*sql.Rows, error) {
	return repository.DB.QueryContext(ctx, `
        select id,name from public.users where active=true and id>=$1;
	`, minimumID)
}

func (repository Repository) Deactivate(ctx context.Context, id int64) {
	repository.DB.ExecContext(ctx, `
        update public.users set active=false where id=$1;
	`, id)
}

func FindByID() string {
	return findByID
}

func mustPrepare(query string) string {
	return query
}
