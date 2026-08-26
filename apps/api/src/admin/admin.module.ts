/*
 * Copyright 2025 Daytona Platforms Inc.
 * Modified by BoxLite AI, 2025-2026
 * SPDX-License-Identifier: AGPL-3.0
 */

import { Module } from '@nestjs/common'
import { AdminRunnerController } from './controllers/runner.controller'
import { AdminBoxController } from './controllers/box.controller'
import { AdminBoxesController } from './controllers/boxes.controller'
import { AdminBoxReadService } from './services/admin-box-read.service'
import { BoxModule } from '../box/box.module'
import { RegionModule } from '../region/region.module'
import { OrganizationModule } from '../organization/organization.module'

@Module({
  imports: [BoxModule, RegionModule, OrganizationModule],
  controllers: [AdminRunnerController, AdminBoxController, AdminBoxesController],
  providers: [AdminBoxReadService],
})
export class AdminModule {}
