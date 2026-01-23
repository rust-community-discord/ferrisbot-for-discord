-- Add migration script here
create table if not exists highlights (
    id integer primary key autoincrement not null,
    member_id integer not null,
    highlight text not null,
    unique (member_id, highlight)
);
