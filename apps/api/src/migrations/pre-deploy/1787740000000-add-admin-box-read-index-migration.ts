import { MigrationInterface, QueryRunner } from 'typeorm'

export class AddAdminBoxReadIndex1787740000000 implements MigrationInterface {
  name = 'AddAdminBoxReadIndex1787740000000'

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`CREATE INDEX "box_updatedat_id_idx" ON "box" ("updatedAt" DESC, "id" DESC)`)
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`DROP INDEX "box_updatedat_id_idx"`)
  }
}
