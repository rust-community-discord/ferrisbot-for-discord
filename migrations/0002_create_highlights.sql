-- Add migration script here
create table if not exists highlights (
    id integer not null,
    highlight text not null,
    primary key (id, highlight)
);
