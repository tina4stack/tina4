# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.115"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.115/tina4-darwin-arm64"
      sha256 "b73e9cd5f0e330d65c82e4304694c226dc5f874cfe84e406906894be072d72ba"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.115/tina4-darwin-amd64"
      sha256 "1a31595d59cac33d1f64355d8a761b3adabfcf5d556676f498580ecc55622a47"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.115/tina4-linux-arm64"
      sha256 "216563bddd878fb227372f333d171971c5bbe4b110bc35ec8b4cbcd3336ba692"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.115/tina4-linux-amd64"
      sha256 "f1dfb665dc10da89f79ba5effcec3d643d83e3cbf10adb841f770dc2ef65e3a7"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
