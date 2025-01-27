# Copyright Materialize, Inc. and contributors. All rights reserved.
#
# Use of this software is governed by the Business Source License
# included in the LICENSE file at the root of this repository.
#
# As of the Change Date specified in that file, in accordance with
# the Business Source License, use of this software will be governed
# by the Apache License, Version 2.0.
# Copyright Materialize, Inc. and contributors. All rights reserved.
#
# Use of this software is governed by the Business Source License
# included in the LICENSE file at the root of this repository.
#
# As of the Change Date specified in that file, in accordance with
# the Business Source License, use of this software will be governed
# by the Apache License, Version 2.0.

from typing import List

from materialize import util
from materialize.checks.actions import Action, Initialize, Manipulate, Sleep, Validate
from materialize.checks.mzcompose_actions import (
    KillComputed,
    KillMz,
    StartComputed,
    StartMz,
    UseComputed,
)
from materialize.checks.scenarios import Scenario
from materialize.mzcompose.services import Materialized

LAST_RELEASED_VERSION = f"v{util.released_materialize_versions()[0]}"

# Older Mz versions are not configured to know SIZE '4-4' clusters by default
size = Materialized.Size.DEFAULT_SIZE
environment_extra = [
    f'MZ_STORAGE_HOST_SIZES={{"{size}":{{"workers":{size}}}}}',
    f'MZ_CLUSTER_REPLICA_SIZES={{"1":{{"workers":1,"scale":1}},"{size}-{size}":{{"workers":{size},"scale":{size}}}}}',
]


class UpgradeEntireMz(Scenario):
    """Upgrade the entire Mz instance from LAST_RELEASED_VERSION all at once."""

    def actions(self) -> List[Action]:
        return [
            StartMz(tag=LAST_RELEASED_VERSION, environment_extra=environment_extra),
            Initialize(self.checks),
            Manipulate(self.checks, phase=1),
            KillMz(),
            StartMz(tag=None),
            Manipulate(self.checks, phase=2),
            Validate(self.checks),
        ]


#
# We are limited with respect to the different orders in which stuff can be upgraded:
# - some sequences of events are invalid
# - environmentd and storaged are located in the same container
#
# Still, we would like to try as many scenarios as we can
#


class UpgradeComputedLast(Scenario):
    """Upgrade computed separately after upgrading environmentd+storaged"""

    def actions(self) -> List[Action]:
        return [
            StartMz(tag=LAST_RELEASED_VERSION, environment_extra=environment_extra),
            StartComputed(tag=LAST_RELEASED_VERSION),
            UseComputed(),
            Initialize(self.checks),
            Manipulate(self.checks, phase=1),
            KillMz(),
            StartMz(tag=None),
            # No useful work can be done while computed is old-verison
            # and environmentd/storaged is new-version. So we proceed
            # to upgrade computed as well.
            # We sleep here to allow some period of coexistence, even
            # though we are not issuing queries during that time.
            Sleep(10),
            KillComputed(),
            StartComputed(tag=None),
            Manipulate(self.checks, phase=2),
            Validate(self.checks),
        ]


class UpgradeComputedFirst(Scenario):
    """Upgrade computed separately before environmentd and storaged"""

    def actions(self) -> List[Action]:
        return [
            StartMz(tag=LAST_RELEASED_VERSION, environment_extra=environment_extra),
            StartComputed(tag=LAST_RELEASED_VERSION),
            UseComputed(),
            Initialize(self.checks),
            Manipulate(self.checks, phase=1),
            KillComputed(),
            StartComputed(tag=None),
            # No useful work can be done while computed is new-verison
            # and environmentd/storaged is old-version. So we
            # proceed to upgrade them as well.
            # We sleep here to allow some period of coexistence, even
            # though we are not issuing queries during that time.
            Sleep(10),
            KillMz(),
            StartMz(tag=None),
            Manipulate(self.checks, phase=2),
            Validate(self.checks),
        ]
