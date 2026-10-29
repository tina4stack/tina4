# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.111"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.111/tina4-darwin-arm64"
      sha256 "4f55a2cf8e6206b7e0517605e115a2e6ae34f5b5971d2dd0637e91ef48281bb6"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.111/tina4-darwin-amd64"
      sha256 "184d575bd2c7268330746df973d47e57364ae467add2ee29f063b1d83bef3445"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.111/tina4-linux-arm64"
      sha256 "6bb57f0f8be7c5c5840cc91f514c86a3e551f0b9ae7d394fe2d4a5be9321fb76"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.111/tina4-linux-amd64"
      sha256 "30ffe075fd05485d7da9596560bbb8f8b48f77b102abd16d7b8b6d7e61f25619"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
