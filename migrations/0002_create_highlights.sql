-- Add migration script here
create table if not exists highlights (
    id bigint primary key not null,
    highlight text[] not null
);