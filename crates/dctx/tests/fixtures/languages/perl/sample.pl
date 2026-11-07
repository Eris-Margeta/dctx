package Sample::Boxed;
sub copy {
    my ($value) = @_;
    sub nested { return shift; }
    return nested($value);
}
sub café { return 1; }
