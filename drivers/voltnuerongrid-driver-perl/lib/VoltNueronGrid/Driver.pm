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
transport and L<JSON::PP> (core module) for JSON serialisation.

=cut

sub new {
    my ($class, %args) = @_;
    my $self = {
        base_url    => $args{base_url}    // 'http://localhost:8080',
        admin_key   => $args{admin_key}   // '',
        session_id  => $args{session_id}  // '',
        timeout     => $args{timeout}     // 30,
        _ua         => LWP::UserAgent->new(timeout => $args{timeout} // 30),
    };
    return bless $self, $class;
}

=head2 execute_sql($sql_batch)

Executes a SQL batch string against C</api/v1/sql/execute>.

Returns the decoded JSON response as a hash reference.

=cut

sub execute_sql {
    my ($self, $sql_batch) = @_;
    my $url = $self->{base_url} . '/api/v1/sql/execute';
    my $req = HTTP::Request->new('POST', $url);
    $req->header('Content-Type' => 'application/json');
    $req->header('X-Admin-Api-Key' => $self->{admin_key}) if $self->{admin_key};
    $req->header('X-Session-Id'   => $self->{session_id}) if $self->{session_id};
    $req->content(encode_json({ sql_batch => $sql_batch }));
    my $resp = $self->{_ua}->request($req);
    return decode_json($resp->content);
}

=head2 health()

Calls C</health> and returns the decoded JSON response.

=cut

sub health {
    my ($self) = @_;
    my $resp = $self->{_ua}->get($self->{base_url} . '/health');
    return decode_json($resp->content);
}

1;

__END__

=head1 AUTHOR

VoltNueronGrid contributors.

=head1 LICENSE

MIT

=cut
