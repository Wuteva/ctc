/* ctc-ignore-file no-throw-cpp */

int legacy(bool invalid) {
  if (invalid) {
    throw 2;
  }
  return 0;
}
