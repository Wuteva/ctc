#pragma once

#include <string>

namespace ui {

class Widget {
public:
  Widget();
  explicit Widget(int value);
  ~Widget();
  Widget(const Widget&) = delete;
  int value() const;
  int& value();
  void rename(const std::string& name = "widget");
  void reset();
  virtual void draw() = 0;
  int inlineValue() const { return value_; }
  static int make(int value);
  template <typename T> void visit(T visitor);

  class Part {
  public:
    void attach();
  };

private:
  int value_ = 0;
};

}  // namespace ui
