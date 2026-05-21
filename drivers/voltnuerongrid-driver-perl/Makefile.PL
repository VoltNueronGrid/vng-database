use 5.010;
use strict;
use warnings;
use ExtUtils::MakeMaker;

WriteMakefile(
    NAME             => 'VoltNueronGrid::Driver',
    AUTHOR           => 'VoltNueronGrid contributors',
    VERSION_FROM     => 'lib/VoltNueronGrid/Driver.pm',
    ABSTRACT_FROM    => 'lib/VoltNueronGrid/Driver.pm',
    LICENSE          => 'mit',
    MIN_PERL_VERSION => '5.010',
    PREREQ_PM        => {
        'LWP::UserAgent' => '6.00',
        'HTTP::Request'  => '6.00',
        'JSON::PP'       => '2.27',
    },
    TEST_REQUIRES => {
        'Test::More' => '1.00',
    },
    META_MERGE => {
        'meta-spec' => { version => 2 },
        resources   => {
            repository => {
                type => 'git',
                url  => 'https://github.com/voltnuerongrid/polap-db.git',
            },
        },
    },
    dist  => { COMPRESS => 'gzip -9f', SUFFIX => 'gz' },
    clean => { FILES    => 'VoltNueronGrid-Driver-*' },
);
