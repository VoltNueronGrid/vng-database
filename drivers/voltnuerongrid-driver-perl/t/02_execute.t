#!/usr/bin/env perl
use strict;
use warnings;
use Test::More;
use lib 'lib';

use_ok('VoltNueronGrid::Driver');

# host + port constructor builds base_url.
my $d = VoltNueronGrid::Driver->new(host => '127.0.0.1', port => 9090, admin_key => 'k');
is($d->{base_url}, 'http://127.0.0.1:9090', 'host+port builds base_url');
is($d->{max_retries}, 2, 'default max_retries is 2');

# base_url trailing slash is trimmed.
my $d2 = VoltNueronGrid::Driver->new(base_url => 'http://localhost:8080/');
is($d2->{base_url}, 'http://localhost:8080', 'trailing slash trimmed');

# _normalize_result: columnar rows.
{
    my $decoded = {
        status  => 'ok',
        columns => [ 'id', 'name' ],
        rows    => [ [ '1', 'alice' ], [ '2', 'bob' ] ],
    };
    my $rs = VoltNueronGrid::Driver::_normalize_result($decoded);
    is($rs->{status}, 'ok', 'status preserved');
    is_deeply($rs->{columns}, [ 'id', 'name' ], 'columns parsed');
    is(scalar(@{ $rs->{rows} }), 2, 'two rows parsed');
    is($rs->{rows}[0][1], 'alice', 'cell value parsed');
}

# _normalize_result: object rows infer columns.
{
    my $decoded = { rows => [ { id => '1', name => 'alice' } ] };
    my $rs = VoltNueronGrid::Driver::_normalize_result($decoded);
    ok((grep { $_ eq 'id' } @{ $rs->{columns} }), 'id column inferred');
    ok((grep { $_ eq 'name' } @{ $rs->{columns} }), 'name column inferred');
    is(scalar(@{ $rs->{rows} }), 1, 'one object row parsed');
}

# _normalize_result: undef/scalar cells stringified safely.
{
    my $decoded = { columns => [ 'n' ], rows => [ [ undef ], [ 5 ] ] };
    my $rs = VoltNueronGrid::Driver::_normalize_result($decoded);
    is($rs->{rows}[0][0], '', 'undef cell becomes empty string');
    is($rs->{rows}[1][0], '5', 'numeric cell stringified');
}

# execute() validates empty SQL by dying with a structured error.
{
    my $err;
    eval { $d->execute(''); };
    $err = $@;
    ok(ref $err eq 'HASH', 'execute dies with a hashref error');
    is($err->{code}, 'validation', 'empty SQL yields validation error code');
}

# execute() with a mock UA returning a 200 JSON body (no live server).
{
    my $mock_ua = MockUA->new(
        code    => 200,
        content => '{"status":"ok","columns":["id"],"rows":[["7"]]}',
    );
    my $md = VoltNueronGrid::Driver->new(base_url => 'http://x', _ua => $mock_ua);
    my $rs = $md->execute('SELECT id FROM t');
    is($rs->{rows}[0][0], '7', 'execute parses mock 200 response');
}

# execute() dies on non-2xx with http_status error.
{
    my $mock_ua = MockUA->new(code => 500, content => 'boom');
    my $md = VoltNueronGrid::Driver->new(base_url => 'http://x', _ua => $mock_ua, max_retries => 0);
    eval { $md->execute('SELECT 1'); };
    my $err = $@;
    is($err->{code}, 'http_status', 'non-2xx yields http_status error');
    is($err->{status_code}, 500, 'status code propagated');
}

done_testing();

# --- Minimal mock LWP::UserAgent for offline tests ------------------------
package MockUA;
sub new { my ($c, %a) = @_; return bless { %a }, $c }
sub request { my ($self) = @_; return MockResp->new($self->{code}, $self->{content}) }
sub get     { my ($self) = @_; return MockResp->new($self->{code}, $self->{content}) }

package MockResp;
sub new { my ($c, $code, $content) = @_; return bless { code => $code, content => $content }, $c }
sub is_success { my ($self) = @_; return $self->{code} >= 200 && $self->{code} < 300 }
sub code    { $_[0]->{code} }
sub content { $_[0]->{content} }
