#include <memory>

class Widget {
public:
  explicit Widget(int value) : value_(value) {}
  int value() const { return value_; }

private:
  int value_;
};

std::unique_ptr<Widget> createWidget(int value) {
  return std::make_unique<Widget>(value);
}
