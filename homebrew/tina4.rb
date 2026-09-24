# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.91"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.91/tina4-darwin-arm64"
      sha256 "97fb2795accd7e1452f208bfa0135c78cec6b4a062667ea85c67d3ee4c623e97"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.91/tina4-darwin-amd64"
      sha256 "953a3d971c92bb30e820b42dfa9e478857f03214c77d03b69d6a09c8912d0a26"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.91/tina4-linux-arm64"
      sha256 "3d61c67b7d583b316c11bd1b24b9e1a5cb324b55647a077a96c9c7069e47450b"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.91/tina4-linux-amd64"
      sha256 "c0876ef9daac5c8d8d078c99e5a43e1f2d4a17a906b9c3ed2b58459f97631232"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
