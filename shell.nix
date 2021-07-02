let
  src = builtins.fetchGit {
    url = "git@github.com:holochain/holonix";
    rev = "014d28000c8ed021eb84000edfe260c22e90af8c";
  };
in

import src {}
