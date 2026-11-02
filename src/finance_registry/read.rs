use super::*;

#[derive(Serialize)]
struct Company {
    id: i64,
    code: String,
    name: String,
}
#[derive(Serialize)]
struct Branch {
    id: i64,
    company_id: i64,
    code: String,
    name: String,
}
#[derive(Serialize)]
struct Scope {
    account_id: i64,
    company_id: i64,
    code: String,
    name: String,
    currency: String,
}
#[derive(Serialize)]
struct Entity {
    id: i64,
    company_id: i64,
    branch_id: i64,
    account_id: i64,
    name: String,
    kind: String,
    currency: String,
}
#[derive(Serialize)]
struct Registry {
    companies: Vec<Company>,
    branches: Vec<Branch>,
    scopes: Vec<Scope>,
    parties: Vec<Entity>,
    funds: Vec<Entity>,
}
fn load(db: &Connection, preview: bool, actor: &Principal) -> Result<Registry, Error> {
    let tx = db.unchecked_transaction()?;
    if !preview && !crate::identity::is_current_user(&tx, actor)? {
        return Err(Error::Unauthorized);
    }
    let viewer = actor.id;
    let all = i64::from(preview);
    let companies = {
        let mut st = tx.prepare("SELECT c.id,c.code,c.name FROM finance_companies c
            WHERE ?1=1 OR EXISTS(SELECT 1 FROM finance_company_admins g WHERE g.company_id=c.id AND g.user_id=?2)
            OR EXISTS(SELECT 1 FROM finance_branch_grants g WHERE g.company_id=c.id AND g.user_id=?2)
            ORDER BY c.code,c.id")?;
        let rows = st.query_map(params![all, viewer], |r| {
            Ok(Company {
                id: r.get(0)?,
                code: r.get(1)?,
                name: r.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let branches = {
        let mut st = tx.prepare(
            "SELECT b.id,b.company_id,b.code,b.name FROM finance_branches b
             WHERE ?1=1 OR EXISTS(SELECT 1 FROM finance_branch_grants g WHERE g.company_id=b.company_id AND g.branch_id=b.id AND g.user_id=?2)
             ORDER BY b.company_id,b.code,b.id",
        )?;
        let rows = st.query_map(params![all, viewer], |r| {
            Ok(Branch {
                id: r.get(0)?,
                company_id: r.get(1)?,
                code: r.get(2)?,
                name: r.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let scopes = {
        let mut st=tx.prepare("SELECT s.account_id,s.company_id,COALESCE(c.code,a.code),a.name,a.currency FROM finance_account_scopes s JOIN accounts a ON a.id=s.account_id LEFT JOIN finance_company_account_codes c ON c.account_id=a.id
        WHERE ?1=1 OR EXISTS(SELECT 1 FROM finance_company_admins g WHERE g.company_id=s.company_id AND g.user_id=?2)
        OR EXISTS(SELECT 1 FROM finance_branch_grants g WHERE g.company_id=s.company_id AND g.user_id=?2)
        ORDER BY a.code,a.id")?;
        let rows = st.query_map(params![all, viewer], |r| {
            Ok(Scope {
                account_id: r.get(0)?,
                company_id: r.get(1)?,
                code: r.get(2)?,
                name: r.get(3)?,
                currency: r.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    fn entities(
        tx: &Connection,
        sql: &str,
        all: i64,
        viewer: i64,
    ) -> rusqlite::Result<Vec<Entity>> {
        let mut st = tx.prepare(sql)?;
        let rows = st.query_map(params![all, viewer], |r| {
            Ok(Entity {
                id: r.get(0)?,
                company_id: r.get(1)?,
                branch_id: r.get(2)?,
                account_id: r.get(3)?,
                name: r.get(4)?,
                kind: r.get(5)?,
                currency: r.get(6)?,
            })
        })?;
        rows.collect()
    }
    let parties = entities(
        &tx,
        "SELECT p.id,p.company_id,p.branch_id,p.account_id,p.name,p.role,a.currency FROM finance_parties p JOIN accounts a ON a.id=p.account_id
         WHERE ?1=1 OR EXISTS(SELECT 1 FROM finance_branch_grants g WHERE g.user_id=?2 AND g.company_id=p.company_id AND g.branch_id=p.branch_id)
         ORDER BY p.id",
        all, viewer,
    )?;
    let funds = entities(
        &tx,
        "SELECT f.id,f.company_id,f.branch_id,f.account_id,f.name,f.kind,a.currency FROM finance_funds f JOIN accounts a ON a.id=f.account_id
         WHERE ?1=1 OR EXISTS(SELECT 1 FROM finance_branch_grants g WHERE g.user_id=?2 AND g.company_id=f.company_id AND g.branch_id=f.branch_id)
         ORDER BY f.id",
        all, viewer,
    )?;
    tx.commit()?;
    Ok(Registry {
        companies,
        branches,
        scopes,
        parties,
        funds,
    })
}
pub async fn list(
    State(state): State<AppState>,
    Extension(actor): Extension<Principal>,
) -> Response {
    let result = db_open(&state.db)
        .map_err(Error::Storage)
        .and_then(|db| load(&db, state.preview_no_login, &actor));
    match result {
        Ok(data) => Json(data).into_response(),
        Err(e) => error_response(e),
    }
}
