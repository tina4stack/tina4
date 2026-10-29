# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.109"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.109/tina4-darwin-arm64"
      sha256 "1ad3fde2a4d38fcc44a1c691beba06bf3bde954baba14043e101e58d2adb6d8f"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.109/tina4-darwin-amd64"
      sha256 "014c107154b231dac3f5a8a2860ee5337afad47e4accf17f75321fce6c13cbed"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.109/tina4-linux-arm64"
      sha256 "e70c9b6ac007b63c812bb8e391cdb66b1470420193d7f39cf22359adc1f2f363"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.109/tina4-linux-amd64"
      sha256 "34fad5291586570430bccc98e066cb906a1820344b710cff5c388feb4d54c9b5"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
