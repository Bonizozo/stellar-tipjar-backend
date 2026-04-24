CREATE TABLE teams (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    owner_username TEXT NOT NULL REFERENCES creators(username) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE team_members (
    team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    creator_username TEXT NOT NULL REFERENCES creators(username) ON DELETE CASCADE,
    share_percentage INT NOT NULL CHECK (share_percentage > 0),
    added_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (team_id, creator_username)
);

CREATE UNIQUE INDEX IF NOT EXISTS team_members_creator_unique ON team_members(creator_username);

CREATE TABLE tip_splits (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tip_id UUID NOT NULL REFERENCES tips(id) ON DELETE CASCADE,
    team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    member_username TEXT NOT NULL REFERENCES creators(username) ON DELETE CASCADE,
    amount TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
