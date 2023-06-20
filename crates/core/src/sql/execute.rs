use spacetimedb_lib::{ProductType, ProductValue};
use spacetimedb_sats::relation::MemTable;
use spacetimedb_vm::eval::run_ast;
use spacetimedb_vm::expr::{CodeResult, CrudExpr, Expr};

use crate::database_instance_context_controller::DatabaseInstanceContextController;
use crate::db::relational_db::RelationalDB;
use crate::error::{DBError, DatabaseError};
use crate::sql::compiler::compile;
use crate::vm::DbProgram;

pub struct StmtResult {
    pub schema: ProductType,
    pub rows: Vec<ProductValue>,
}

// TODO(cloutiertyler): we could do this the swift parsing way in which
// we always generate a plan, but it may contain errors

/// Run a `SQL` query/statement in the specified `database_instance_id`.
pub fn execute(
    db_inst_ctx_controller: &DatabaseInstanceContextController,
    database_instance_id: u64,
    sql_text: String,
) -> Result<Vec<MemTable>, DBError> {
    if let Some((database_instance_context, _)) = db_inst_ctx_controller.get(database_instance_id) {
        run(&database_instance_context.relational_db, &sql_text)
    } else {
        Err(DatabaseError::NotFound(database_instance_id).into())
    }
}

fn collect_result(result: &mut Vec<MemTable>, r: CodeResult) -> Result<(), DBError> {
    match r {
        CodeResult::Value(_) => {}
        CodeResult::Table(x) => result.push(x),
        CodeResult::Block(lines) => {
            for x in lines {
                collect_result(result, x)?;
            }
        }
        CodeResult::Halt(err) => return Err(DBError::VmUser(err)),
        CodeResult::Pass => {}
    }

    Ok(())
}

pub fn compile_sql(db: &RelationalDB, sql_text: &str) -> Result<Vec<CrudExpr>, DBError> {
    compile(db, sql_text)
}

pub fn execute_single_sql(db: &RelationalDB, ast: CrudExpr) -> Result<Vec<MemTable>, DBError> {
    let p = &mut DbProgram::new(db.clone());
    let q = Expr::Crud(Box::new(ast));

    let mut result = Vec::with_capacity(1);
    collect_result(&mut result, run_ast(p, q).into())?;
    Ok(result)
}

pub fn execute_sql(db: &RelationalDB, ast: Vec<CrudExpr>) -> Result<Vec<MemTable>, DBError> {
    let total = ast.len();

    let p = &mut DbProgram::new(db.clone());
    let q = Expr::Block(ast.into_iter().map(|x| Expr::Crud(Box::new(x))).collect());

    let mut result = Vec::with_capacity(total);
    collect_result(&mut result, run_ast(p, q).into())?;
    Ok(result)
}

fn run(db: &RelationalDB, sql_text: &str) -> Result<Vec<MemTable>, DBError> {
    let ast = compile_sql(db, sql_text)?;
    execute_sql(db, ast)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::relational_db::tests_utils::make_test_db;
    use crate::db::relational_db::{ST_TABLES_ID, ST_TABLES_NAME};
    use crate::vm::tests::create_table_with_rows;
    use spacetimedb_lib::error::ResultTest;
    use spacetimedb_sats::relation::Header;
    use spacetimedb_sats::{product, BuiltinType, ProductType};
    use spacetimedb_vm::dsl::{mem_table, scalar};
    use spacetimedb_vm::eval::create_game_data;
    use tempdir::TempDir;

    fn create_data(total_rows: u64) -> ResultTest<(RelationalDB, MemTable, TempDir)> {
        let (db, tmp_dir) = make_test_db()?;

        let head = ProductType::from_iter([("inventory_id", BuiltinType::U64), ("name", BuiltinType::String)]);
        let rows: Vec<_> = (1..=total_rows).map(|i| product!(i, format!("health{i}"))).collect();
        create_table_with_rows(&db, "inventory", head.clone(), &rows)?;

        Ok((db, mem_table(head, rows), tmp_dir))
    }

    #[test]
    fn test_select_star() -> ResultTest<()> {
        let (db, input, _tmp_dir) = create_data(1)?;
        let result = run(&db, "SELECT * FROM inventory")?;

        assert_eq!(result.len(), 1, "Not return results");
        let result = result.first().unwrap().clone();

        assert_eq!(
            result.as_without_table_name(),
            input.as_without_table_name(),
            "Inventory"
        );
        Ok(())
    }

    #[test]
    fn test_select_star_table() -> ResultTest<()> {
        let (db, input, _tmp_dir) = create_data(1)?;

        let result = run(&db, "SELECT inventory.* FROM inventory")?;
        assert_eq!(result.len(), 1, "Not return results");
        let result = result.first().unwrap().clone();

        assert_eq!(
            result.as_without_table_name(),
            input.as_without_table_name(),
            "Inventory"
        );

        let result = run(
            &db,
            "SELECT inventory.inventory_id FROM inventory WHERE inventory.inventory_id = 1",
        )?;
        assert_eq!(result.len(), 1, "Not return results");
        let result = result.first().unwrap().clone();

        let head = ProductType::from_iter([("inventory_id", BuiltinType::U64)]);
        let row = product!(1u64);
        let input = mem_table(head, vec![row]);

        assert_eq!(
            result.as_without_table_name(),
            input.as_without_table_name(),
            "Inventory"
        );

        Ok(())
    }

    #[test]
    fn test_select_scalar() -> ResultTest<()> {
        let (db, _, _tmp_dir) = create_data(1)?;
        let result = run(&db, "SELECT 1 FROM inventory")?;

        assert_eq!(result.len(), 1, "Not return results");
        let result = result.first().unwrap().clone();
        let schema = ProductType::from_iter([BuiltinType::I32]);
        let row = product!(scalar(1));
        let input = mem_table(schema, vec![row]);

        assert_eq!(result.as_without_table_name(), input.as_without_table_name(), "Scalar");
        Ok(())
    }

    #[test]
    fn test_select_multiple() -> ResultTest<()> {
        let (db, input, _tmp_dir) = create_data(1)?;
        let result = run(&db, "SELECT * FROM inventory;\nSELECT * FROM inventory")?;

        assert_eq!(result.len(), 2, "Not return results");

        for x in result {
            assert_eq!(x.as_without_table_name(), input.as_without_table_name(), "Inventory");
        }
        Ok(())
    }

    #[test]
    fn test_select_catalog() -> ResultTest<()> {
        let (db, _, _tmp_dir) = create_data(1)?;
        let tx = db.begin_tx();
        let schema = db.schema_for_table(&tx, ST_TABLES_ID).unwrap();
        db.rollback_tx(tx);

        let result = run(
            &db,
            &format!("SELECT * FROM {} WHERE table_id = {}", ST_TABLES_NAME, ST_TABLES_ID),
        )?;

        assert_eq!(result.len(), 1, "Not return results");
        let result = result.first().unwrap().clone();
        let row = product!(scalar(ST_TABLES_ID), scalar(ST_TABLES_NAME), scalar(true));
        let input = mem_table(Header::from(&schema), vec![row]);

        assert_eq!(
            result.as_without_table_name(),
            input.as_without_table_name(),
            "st_table"
        );
        Ok(())
    }

    #[test]
    fn test_select_column() -> ResultTest<()> {
        let (db, table, _tmp_dir) = create_data(1)?;
        let result = run(&db, "SELECT inventory_id FROM inventory")?;

        assert_eq!(result.len(), 1, "Not return results");
        let result = result.first().unwrap().clone();
        //The expected result
        let col = table.head.find_by_name("inventory_id").unwrap();
        let inv = table.head.project(&[col.field.clone()]).unwrap();

        let row = product!(scalar(1u64));
        let input = mem_table(inv, vec![row]);

        assert_eq!(
            result.as_without_table_name(),
            input.as_without_table_name(),
            "Inventory"
        );
        Ok(())
    }

    #[test]
    fn test_where() -> ResultTest<()> {
        let (db, table, _tmp_dir) = create_data(1)?;
        let result = run(&db, "SELECT inventory_id FROM inventory WHERE inventory_id = 1")?;

        assert_eq!(result.len(), 1, "Not return results");
        let result = result.first().unwrap().clone();

        //The expected result
        let col = table.head.find_by_name("inventory_id").unwrap();
        let inv = table.head.project(&[col.field.clone()]).unwrap();

        let row = product!(scalar(1u64));
        let input = mem_table(inv, vec![row]);

        assert_eq!(
            result.as_without_table_name(),
            input.as_without_table_name(),
            "Inventory"
        );
        Ok(())
    }

    #[test]
    fn test_or() -> ResultTest<()> {
        let (db, table, _tmp_dir) = create_data(2)?;

        let result = run(
            &db,
            "SELECT inventory_id FROM inventory WHERE inventory_id = 1 OR inventory_id = 2",
        )?;

        assert_eq!(result.len(), 1, "Not return results");
        let mut result = result.first().unwrap().clone();
        result.data.sort();
        //The expected result
        let col = table.head.find_by_name("inventory_id").unwrap();
        let inv = table.head.project(&[col.field.clone()]).unwrap();

        let input = mem_table(inv, vec![product!(scalar(1u64)), product!(scalar(2u64))]);

        assert_eq!(
            result.as_without_table_name(),
            input.as_without_table_name(),
            "Inventory"
        );
        Ok(())
    }

    #[test]
    fn test_nested() -> ResultTest<()> {
        let (db, table, _tmp_dir) = create_data(2)?;

        let result = run(
            &db,
            "SELECT (inventory_id) FROM inventory WHERE (inventory_id = 1 OR inventory_id = 2 AND (1=1))",
        )?;

        assert_eq!(result.len(), 1, "Not return results");
        let mut result = result.first().unwrap().clone();
        result.data.sort();
        //The expected result
        let col = table.head.find_by_name("inventory_id").unwrap();
        let inv = table.head.project(&[col.field.clone()]).unwrap();

        let input = mem_table(inv, vec![product!(scalar(1u64)), product!(scalar(2u64))]);

        assert_eq!(
            result.as_without_table_name(),
            input.as_without_table_name(),
            "Inventory"
        );
        Ok(())
    }

    #[test]
    fn test_inner_join() -> ResultTest<()> {
        let data = create_game_data();

        let (db, _tmp_dir) = make_test_db()?;

        create_table_with_rows(&db, "Inventory", data.inv.head.into(), &data.inv.data)?;
        create_table_with_rows(&db, "Player", data.player.head.into(), &data.player.data)?;
        create_table_with_rows(&db, "Location", data.location.head.into(), &data.location.data)?;

        let result = &run(
            &db,
            "SELECT
        Player.*
            FROM
        Player
        JOIN Location
        ON Location.entity_id = Player.entity_id
        WHERE x > 0 AND x <= 32 AND z > 0 AND z <= 32",
        )?[0];

        let head = ProductType::from_iter([("entity_id", BuiltinType::U64), ("inventory_id", BuiltinType::U64)]);
        let row1 = product!(100u64, 1u64);
        let input = mem_table(head, [row1]);

        assert_eq!(
            input.as_without_table_name(),
            result.as_without_table_name(),
            "Player JOIN Location"
        );

        let result = &run(
            &db,
            "SELECT
        Inventory.*
            FROM
        Inventory
        JOIN Player
        ON Inventory.inventory_id = Player.inventory_id
        JOIN Location
        ON Player.entity_id = Location.entity_id
        WHERE x > 0 AND x <= 32 AND z > 0 AND z <= 32",
        )?[0];

        let head = ProductType::from_iter([("inventory_id", BuiltinType::U64), ("name", BuiltinType::String)]);
        let row1 = product!(1u64, "health");
        let input = mem_table(head, [row1]);

        assert_eq!(
            input.as_without_table_name(),
            result.as_without_table_name(),
            "Inventory JOIN Player JOIN Location"
        );
        Ok(())
    }

    #[test]
    fn test_insert() -> ResultTest<()> {
        let (db, mut input, _tmp_dir) = create_data(1)?;
        let result = run(&db, "INSERT INTO inventory (inventory_id, name) VALUES (2, 'test')")?;

        assert_eq!(result.len(), 0, "Return results");

        let result = run(&db, "SELECT * FROM inventory")?;

        assert_eq!(result.len(), 1, "Not return results");
        let mut result = result.first().unwrap().clone();

        let row = product!(scalar(2u64), scalar("test"));
        input.data.push(row);
        input.data.sort();
        result.data.sort();

        assert_eq!(
            result.as_without_table_name(),
            input.as_without_table_name(),
            "Inventory"
        );

        Ok(())
    }

    #[test]
    fn test_delete() -> ResultTest<()> {
        let (db, _input, _tmp_dir) = create_data(1)?;
        run(&db, "INSERT INTO inventory (inventory_id, name) VALUES (2, 't2')")?;
        run(&db, "INSERT INTO inventory (inventory_id, name) VALUES (3, 't3')")?;

        let result = run(&db, "SELECT * FROM inventory")?;
        assert_eq!(
            result.iter().map(|x| x.data.len()).sum::<usize>(),
            3,
            "Not return results"
        );

        run(&db, "DELETE FROM inventory WHERE inventory.inventory_id = 3")?;

        let result = run(&db, "SELECT * FROM inventory")?;
        assert_eq!(
            result.iter().map(|x| x.data.len()).sum::<usize>(),
            2,
            "Not delete correct row?"
        );

        run(&db, "DELETE FROM inventory")?;

        let result = run(&db, "SELECT * FROM inventory")?;
        assert_eq!(
            result.iter().map(|x| x.data.len()).sum::<usize>(),
            0,
            "Not delete all rows"
        );

        Ok(())
    }

    #[test]
    fn test_update() -> ResultTest<()> {
        let (db, input, _tmp_dir) = create_data(1)?;
        run(&db, "INSERT INTO inventory (inventory_id, name) VALUES (2, 't2')")?;
        run(&db, "INSERT INTO inventory (inventory_id, name) VALUES (3, 't3')")?;

        run(&db, "UPDATE inventory SET name = 'c2' WHERE inventory_id = 2")?;

        let result = run(&db, "SELECT * FROM inventory WHERE inventory_id = 2")?;

        let result = result.first().unwrap().clone();
        let row = product!(scalar(2u64), scalar("c2"));

        let mut change = input;
        change.data.clear();
        change.data.push(row);

        assert_eq!(
            change.as_without_table_name(),
            result.as_without_table_name(),
            "Update Inventory 2"
        );

        run(&db, "UPDATE inventory SET name = 'c3'")?;

        let result = run(&db, "SELECT * FROM inventory")?;

        let updated: Vec<_> = result
            .into_iter()
            .map(|x| {
                x.data
                    .into_iter()
                    .map(|x| x.field_as_str(1, None).unwrap().to_string())
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(vec![vec!["c3"; 3]], updated);

        Ok(())
    }

    #[test]
    fn test_create_table() -> ResultTest<()> {
        let (db, _, _tmp_dir) = create_data(1)?;
        run(&db, "CREATE TABLE inventory2 (inventory_id BIGINT UNSIGNED, name TEXT)")?;
        run(
            &db,
            "INSERT INTO inventory2 (inventory_id, name) VALUES (1, 'health1') ",
        )?;

        let a = run(&db, "SELECT * FROM inventory")?;
        let a = a.first().unwrap().clone();

        let b = run(&db, "SELECT * FROM inventory2")?;
        let b = b.first().unwrap().clone();

        assert_eq!(a.as_without_table_name(), b.as_without_table_name(), "Inventory");

        Ok(())
    }

    #[test]
    fn test_drop_table() -> ResultTest<()> {
        let (db, _, _tmp_dir) = create_data(1)?;
        run(&db, "CREATE TABLE inventory2 (inventory_id BIGINT UNSIGNED, name TEXT)")?;
        run(&db, "DROP TABLE inventory2")?;
        match run(&db, "SELECT * FROM inventory2") {
            Ok(_) => {
                panic!("Fail to drop table");
            }
            Err(err) => {
                let msg = err.to_string();
                assert_eq!(
                    "SqlError: Unknown table: `inventory2`, executing: `SELECT * FROM inventory2`",
                    msg
                );
            }
        }

        Ok(())
    }
}