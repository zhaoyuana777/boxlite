import { MigrationInterface, QueryRunner } from 'typeorm'

export class AddBoxRecoveryStartedAt1789100000000 implements MigrationInterface {
  name = 'AddBoxRecoveryStartedAt1789100000000'

  async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`ALTER TABLE "box" ADD "recoveryStartedAt" timestamptz`)
  }

  async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`ALTER TABLE "box" DROP COLUMN "recoveryStartedAt"`)
  }
}
