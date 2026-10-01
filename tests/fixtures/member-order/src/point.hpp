class Forward;
struct Later;

struct Point {
  Point();
  int x() const;

 private:
  int x_;
};

struct PrivateFirst {
 private:
  int hidden_;

 public:
  int shown() const;
};

template <typename T>
struct Holder final : Base {
  using Value = T;

  Holder();
  T get() const;
};

class Outer {
 public:
  struct Inner {
    int value() const;
    Inner();
  };
};
