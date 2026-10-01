int parse(bool invalid) {
  if (invalid) {
    // ctc-ignore-next-line no-throw-cpp -- callers catch this
    throw 1;
  }
  return 0;
}
