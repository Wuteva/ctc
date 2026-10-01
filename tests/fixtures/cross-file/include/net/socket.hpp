#pragma once

namespace net {

class Socket {
public:
  void open(const char* host, int port);
  void close() noexcept;
};

}  // namespace net
