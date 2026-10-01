class OrderedShape {
  friend class ShapeFactory;

public:
  OrderedShape();
  OrderedShape(const OrderedShape&) = default;
  virtual ~OrderedShape();
  OrderedShape& operator=(const OrderedShape&) = default;
  explicit operator bool() const;
  virtual double area() const = 0;
  template <typename T> T as() const;

private:
  int id_;
};

namespace geometry {
class DestructorFirst {
public:
  ~DestructorFirst();
  DestructorFirst();
};

class FriendLast {
public:
  FriendLast();

private:
  int id_;
  friend class ShapeFactory;
};
}  // namespace geometry
