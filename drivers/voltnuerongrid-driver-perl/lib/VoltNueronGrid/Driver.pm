package VoltNueronGrid::Driver;
use strict;
use warnings;
use LWP::UserAgent;
use HTTP::Request;
use JSON::PP qw(encode_json decode_json);

our $VERSION = '0.1.0';

=head1 NAME

VoltNueronGrid::Driver - Perl client driver for the VoltNueronGrid HTTP API.

=head1 SYNOPSIS

  use VoltNueronGrid::Driver;

  my $driver = VoltNueronGrid::Driver->new(
      base_url  => 'http://localhost:8080',
      admin_key => 'secret',
      session_id => 'sess-1',
  );

  my $result = $driver->execute_sql('SELECT * FROM orders');

=head1 DESCRIPTION

Lightweight Perl driver for VoltNueronGrid. Uses L<LWP::UserAgent> for HTTP
transport and L<JSON::PP> (core module) for JSON serialisation. Errors are
reported by C<die>-ing with a structured hash reference (see L</ERROR HANDLING>).

=cut

sub new {
    my ($class, %args) = @_;
    # Accept either an explicit base_url or host+port.
    my $base_url = $args{base_url};
    if (!$base_url && $args{host}) {
        my $port = $args{port} // 8080;
        $base_url = "http://$args{host}:$port";
    }
    $base_url //= 'http://localhost:8080';
    $base_url =~ s{/+$}{};

    my $self = {
        base_url    => $base_url,
        admin_key   => $args{admin_key}   // '',
        session_id  => $args{session_id}  // '',
        timeout     => $args{timeout}     // 30,
        max_retries => $args{max_retries} // 2,
        _ua         => $args{_ua} // LWP::UserAgent->new(timeout => $args{timeout} // 30),
    };
    return bless $self, $class;
}

=head2 execute($sql)

Executes a SQL batch and returns a normalised result hash reference:

  { status => 'ok', columns => [...], rows => [ [...], ... ], raw => {...} }

Retries on HTTP 503 up to C<max_retries> times. Dies with a structured error
(see L</ERROR HANDLING>) on transport failure or a non-2xx HTTP response.

=cut

sub execute {
    my ($self, $sql) = @_;
    die { code => 'validation', message => 'sql must be non-empty' }
        if !defined $sql || $sql eq '';

    my $url = $self->{base_url} . '/api/v1/sql/execute';
    my $resp = $self->_request_with_retry('POST', $url, { sql_batch => $sql });

    if (!$resp->is_success) {
        die {
            code        => 'http_status',
            status_code => $resp->code,
            message     => 'sql/execute failed: HTTP ' . $resp->code,
            body        => $resp->content,
        };
    }

    my $decoded = eval { decode_json($resp->content) };
    if ($@) {
        die { code => 'decode', message => "invalid JSON response: $@" };
    }
    return _normalize_result($decoded);
}

=head2 execute_sql($sql_batch)

Backwards-compatible alias that returns the raw decoded JSON response hash.

=cut

sub execute_sql {
    my ($self, $sql_batch) = @_;
    my $rs = $self->execute($sql_batch);
    return $rs->{raw};
}

=head2 health()

Calls C</health> and returns the decoded JSON response.

=cut

sub health {
    my ($self) = @_;
    my $resp = $self->{_ua}->get($self->{base_url} . '/health');
    die { code => 'http_status', status_code => $resp->code, message => 'health failed' }
        if !$resp->is_success;
    return decode_json($resp->content);
}

# -- internal helpers -------------------------------------------------------

sub _build_request {
    my ($self, $method, $url, $payload) = @_;
    my $req = HTTP::Request->new($method, $url);
    $req->header('Content-Type'    => 'application/json');
    $req->header('X-Admin-Api-Key' => $self->{admin_key})  if $self->{admin_key};
    $req->header('X-Session-Id'    => $self->{session_id}) if $self->{session_id};
    $req->content(encode_json($payload)) if defined $payload;
    return $req;
}

sub _request_with_retry {
    my ($self, $method, $url, $payload) = @_;
    my $attempts = ($self->{max_retries} // 0) + 1;
    my $resp;
    for my $attempt (1 .. $attempts) {
        my $req = $self->_build_request($method, $url, $payload);
        $resp = $self->{_ua}->request($req);
        last if !$resp || $resp->code != 503;
        select(undef, undef, undef, 0.05 * $attempt) if $attempt < $attempts;
    }
    die { code => 'transport', message => 'no response received' } if !$resp;
    return $resp;
}

=head2 _normalize_result($decoded)

Pure function (no I/O) that turns a decoded C<sql/execute> response into the
normalised C<{ status, columns, rows, raw }> shape. Handles both columnar rows
(C<rows =E<gt> [[...],...]>) and object rows (C<rows =E<gt> [{...},...]>);
scalar cells are stringified.

=cut

sub _normalize_result {
    my ($decoded) = @_;
    $decoded = {} if ref $decoded ne 'HASH';

    my @columns = ref $decoded->{columns} eq 'ARRAY'
        ? map { _scalar($_) } @{ $decoded->{columns} }
        : ();

    my @rows;
    if (ref $decoded->{rows} eq 'ARRAY') {
        for my $row (@{ $decoded->{rows} }) {
            if (ref $row eq 'ARRAY') {
                push @rows, [ map { _scalar($_) } @$row ];
            }
            elsif (ref $row eq 'HASH') {
                @columns = sort keys %$row unless @columns;
                push @rows, [ map { _scalar($row->{$_}) } @columns ];
            }
            else {
                push @rows, [ _scalar($row) ];
            }
        }
    }

    return {
        status  => $decoded->{status} // 'ok',
        columns => \@columns,
        rows    => \@rows,
        raw     => $decoded,
    };
}

sub _scalar {
    my ($v) = @_;
    return '' if !defined $v;
    return "$v";
}

1;

__END__

=head1 ERROR HANDLING

Methods C<die> with a hash reference on failure:

  eval { $driver->execute($sql); };
  if (my $err = $@) {
      warn "error code=$err->{code}: $err->{message}";
  }

The C<code> field is one of C<validation>, C<transport>, C<http_status>, or
C<decode>. For C<http_status>, C<status_code> carries the HTTP status.

=head1 AUTHOR

VoltNueronGrid contributors.

=head1 LICENSE

MIT

=cut
