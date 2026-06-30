create table test (
    id int primary key,
    name varchar(255)
);

insert into test (id, name) values (1, 'Alice');
insert into test (id, name) values (2, 'Bob');
insert into test (id, name) values (3, 'Charlie'); 

select * from test;