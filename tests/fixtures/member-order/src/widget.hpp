class OrderedWidget {
  using Id = int;

public:
  OrderedWidget();
  explicit OrderedWidget(int value);
  int value() const;

private:
  int value_;
};

namespace ui {
class ConstructorLast {
public:
  int value() const;
  ConstructorLast();
};
}  // namespace ui
