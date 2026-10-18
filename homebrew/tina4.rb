# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.100"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.100/tina4-darwin-arm64"
      sha256 "5bb66dd44f5ebb6c77b3ec094be196ade3bdfe5b395998aaf47bc36a8786c1a1"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.100/tina4-darwin-amd64"
      sha256 "ec32e8902473c7c0b13095b15b015b236c9bb4ce260db8b3660a9ed58cce335b"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.100/tina4-linux-arm64"
      sha256 "3103071892f5519f71b7c6d17146c5538d63fc26259f1fb26470823cc85c5ad4"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.100/tina4-linux-amd64"
      sha256 "4bbfcfd7a24a3380c3f59e5aa545ebeee6bf8f299713ba8f3e49647e32e25ef4"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
