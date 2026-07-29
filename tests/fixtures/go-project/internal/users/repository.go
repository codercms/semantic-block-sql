package users

import (
	"context"
	"database/sql"
)

const FindByIDQuery = `select id,name,email from public.users where id=$1;`

var ActiveUsersQuery = `
    select id,name,email
    from public.users
    where deleted_at is null
    order by id;
`

type Repository struct {
	DB *sql.DB
}

func (repository Repository) FindByID(ctx context.Context, id int64) (*sql.Rows, error) {
	const auditQuery = `
        select id,created_at from public.user_audit where user_id=$1 order by created_at desc;
    `
	_ = auditQuery

	return repository.DB.QueryContext(ctx, `
        select id,name,email from public.users where id=$1 and deleted_at is null;
    `, id)
}

func (repository Repository) ListActive(ctx context.Context) (*sql.Rows, error) {
	rows, err := repository.DB.QueryContext(ctx, `
        select id,name,email from public.users where deleted_at is null order by id;
    `)
	return rows, err
}

func (repository Repository) Disable(ctx context.Context, id int64) {
	repository.DB.ExecContext(ctx, `
        update public.users set disabled_at=now() where id=$1;
    `, id)
}

func (repository Repository) DeleteExpired(ctx context.Context) {
	repository.DB.ExecContext(ctx, `
        delete from public.users where disabled_at is not null returning id;
    `)
}
