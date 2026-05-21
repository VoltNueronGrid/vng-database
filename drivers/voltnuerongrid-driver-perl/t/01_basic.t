#!/usr/bin/env perl
use strict;
use warnings;
use Test::More;

# Load the driver module from the lib directory.
use lib 'lib';

# Basic load test — verifies the module compiles and exports correctly.
BEGIN { use_ok('VoltNueronGrid::Driver') }

# Constructor smoke test — no server required.
my $driver = VoltNueronGrid::Driver->new(
    base_url   => 'http://localhost:8080',
    admin_key  => 'test-key',
    session_id => 'test-session',
    timeout    => 5,
);

isa_ok($driver, 'VoltNueronGrid::Driver', 'new() returns a VoltNueronGrid::Driver object');
is($driver->{base_url},   'http://localhost:8080', 'base_url is stored correctly');
is($driver->{admin_key},  'test-key',              'admin_key is stored correctly');
is($driver->{session_id}, 'test-session',          'session_id is stored correctly');
is($driver->{timeout},    5,                       'timeout is stored correctly');

# Default-value test.
my $default_driver = VoltNueronGrid::Driver->new();
is($default_driver->{base_url},   'http://localhost:8080', 'default base_url is correct');
is($default_driver->{admin_key},  '',                      'default admin_key is empty string');
is($default_driver->{session_id}, '',                      'default session_id is empty string');
is($default_driver->{timeout},    30,                      'default timeout is 30');

# Verify the internal LWP::UserAgent is properly created.
isa_ok($driver->{_ua}, 'LWP::UserAgent', 'internal _ua is an LWP::UserAgent');

done_testing();
