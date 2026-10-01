#include "widget.hpp"

namespace ui {

Widget::Widget() = default;

Widget::Widget(int value) : value_(value) {}

Widget::~Widget() {}

int Widget::value() const { return value_; }

int& Widget::value() { return value_; }

void Widget::reset() { value_ = 0; }

void Widget::rename(const std::string& name) {}

int Widget::make(long value) { return 0; }

}  // namespace ui

void ui::Widget::Part::attach() {}
