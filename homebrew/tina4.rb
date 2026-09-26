# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.92"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.92/tina4-darwin-arm64"
      sha256 "8162e612afa8674d0387750862b03e2650ee50c9149480924af5ebe70e0b81fe"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.92/tina4-darwin-amd64"
      sha256 "5af1d81b675f0d295463c127a76c36a7d22472ae5ee8fa2f465deb374dab76b1"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.92/tina4-linux-arm64"
      sha256 "22bdd78864b88a6e9b85101de6f5263dc57e3192967535aa5306630672bd53c0"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.92/tina4-linux-amd64"
      sha256 "f3aa5dc5d96d048516c211c9c4b44349e45f1a17895550fed37d5644b4cf453b"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
