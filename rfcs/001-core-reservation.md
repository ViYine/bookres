# Features
* Feature Name: core_reservation
* Start Date: 2022-11-11

## Summary

a core reservation for solve problem of resource reservation.  we leverage postgre EXCLUDE constraint to ensure that only one reservation can be made for a given resource at a given time.

## Motivation

Why are we doing this? What use cases does it support? What is the expected outcome?

we need a common solution for various reservation scenarios, such as : 1) calendar booking; 2) hotel/room/seats bookings, 3) meeting room booking, 4) resource reservation, 5) etc. Reapatedly build a features for these requirements is waste of time and energy. We need a common solution for these scenarios.

## Guide-level explanation

### Service Interface

we use gRPC as the service interface. The service interface is defined as follows:

```proto

syntax = "proto3";

enum ReservationStatus {
    UNKNOW = 0;
    PENDING = 1;
    CONFIRMED = 1;
    BLOCKED = 3;
}

enum ReservationUpdateType {
    UNKNOW = 0;
    CREATE = 1;
    UPDATE = 2;
    DELETE = 3;
}

// when a reservation is canceled, the reservation should be deleted from the database.

message Reservation {
    // reservation id
    string id = 1;
    // user_id is the user who make the reservation
    string user_id = 2;
    // status is the status of reservation, such as pending, confirmed, cancelled, etc. to use show the status of reservation
    ReservationStatus status = 3;

    // resource reservation window
    string resource_id = 4;
    google.protobuf.Timestamp start = 5;
    google.protobuf.Timestamp end = 6;

    // extra note
    string note = 7;
}

message ReserveRequest {
    Reservation reservation = 1;
}

message ReserveResponse {
    Reservation reservation = 1;
}

message UpdateRequest {
    // current just update the note of reservation
    string note = 1;
}

message UpdateResponse {
    Reservation reservation = 1;
}

message ConfirmRequest {
    // given a reservation id, confirm the reservation
    string id = 1;
}

message ConfirmResponse {
    Reservation reservation = 1;
}

message CancelRequest {
    // given a reservation id, cancel the reservation
    string id = 1;
}

message CancelResponse {
    Reservation reservation = 1;
}

message GetRequest {
    // given a reservation id, get the reservation
    string id = 1;
}

message GetResponse {
    Reservation reservation = 1;
}

message QueryRequest {
    // given a resource id, query the reservation
    string resource_id = 1;
    // given a user id, query the reservation
    string user_id = 2;
    // use status to filter the reservation, if status is UNKNOWN, then return all reservation
    ReservationStatus status = 3;
    google.protobuf.Timestamp start = 4;
    google.protobuf.Timestamp end = 5;
}

message WatchRequest {}

message WatchResponse {
    ReservationUpdateType type = 1;
    Reservation reservation = 2;
}

service ReservationService {
    // create a reservation by a reserve, insert a record into database, id should be null
    rpc reserve(ReserveRequest) returns (ReserveResponse);
    // update a reservation, set the note of reservation
    rpc update(UpdateRequest) returns (UpdateRequest);
    // confirm a reservation, update the status of reservation
    rpc confirm(ConfirmRequest) returns (ConfirmResponse);
    // delete a reservation, delete a record from database
    rpc cancel(CancelRequest) returns (CancelResponse);
    // get a reservation, get a record from database
    rpc get(GetRequest) returns (GetResponse);
    // query reservations return a list of reservations by stream
    rpc query(QueryRequest) returns (stream Reservation);
    // another system could monitor the newly added/comfirmed/canceled reservation
    rpc watch(WatchRequest) returns (stream WatchResponse);
}

```

### DataBase Schema

we use postgresql as the database, and we use EXCLUDE constraint to ensure that only one reservation can be made for a given resource at a given time. Below is the schema:

```sql

CREATE SCHEMA rsvp;

CREATE rsvp.reservation_status as ENUM ('unknown','pending', 'confirmed', 'canceled');
CREATE rsvp.reservation_update_type as ENUM ('unknown','create', 'update', 'delete');

CREATE TABLE rsvp.reservations (
    -- reservation id, 这个是自己系统内部维护的id
    id uuid NOT NULL DEFAULT uuid_generate_v4(),
    -- user_id is the user who make the reservation, 这个外部传来的用户id，为了方便，这里用string
    user_id varchar(64) NOT NULL,
    -- status is the status of reservation, such as pending, confirmed, cancelled, etc. to use show the status of reservation
    status rsvp.reservation_status NOT NULL DEFAULT 'pending',
    -- reservation resource id, 这个是外部传来的资源id，为了方便，这里用string
    resource_id varchar(64) NOT NULL,
    -- use a tsrange to represent the reservation window
    timespan tstzrange NOT NULL,
    -- extra note
    note text
    -- add created_at and updated_at
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    -- updated_at can add a trigger to update this field
    updated_at timestamp with time zone NOT NULL DEFAULT now(),

    -- 上面是reservation的基本信息，下面是一些约束
    -- id是主键
    CONSTRAINT reservations_pkey PRIMARY KEY (id),
    -- 保证一个资源在一个时间段内只能有一个预约
    -- 使用gist索引，在resource_id 相等的情况下，使用tsrange的overlaps函数来判断是否有重叠，如果有重叠，则返回false
    CONSTRAINT reservations_conflict EXCLUDE USING gist (resource_id WITH =, timespan WITH &&)
);

-- here is the index for reservation
CREATE INDEX reservations_resource_id_idx ON rsvp.reservations (resource_id);
CREATE INDEX reservations_user_id_idx ON rsvp.reservations (user_id);

-- here is some opteration for reservation use trigger or function
-- query function
-- if use_id is null, find all reservations within during for the resource_id
-- if resource_id is null, find all reservations within during for the user_id
-- if both resource_id and user_id is null, find all reservations within during
-- if both set, find all reservations within during for the resource_id and user_id
CREATE OR REPLACE FUNCTION rsvp.query(uid text, rid text, during tstzrange) RETURNS TABLE rsvp.reservations AS $$ $$ LANGUAGE plpgsql;

-- reservation change queue
CREATE TABLE rsvp.reservation_changes (
    id SERIAL NOT NULL,
    reservation_id uuid NOT NULL,
    op rsvp.reservation_update_type NOT NULL
);

-- tragger for add/update/delete reservation
CREATE OR REPLACE FUNCTION rsvp.reservations_trigger() RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        -- update reservation_changes
        INSERT INTO rsvp.reservation_changes (reservation_id, op) VALUES (NEW.id, 'create');
    ELSIF TG_OP = 'UPDATE' THEN
        -- if the status is changed, update reservation_changes
        IF OLD.status <> NEW.status THEN
            INSERT INTO rsvp.reservation_changes (reservation_id, op) VALUES (NEW.id, 'update');
        END IF;
    ELSIF TG_OP = 'DELETE' THEN
        -- update reservation_changes
        INSERT INTO rsvp.reservation_changes (reservation_id, op) VALUES (OLD.id, 'delete');
    END IF;

    -- notify the reservation_update channel
    NOTIFY reservation_update;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER reservations_trigger
    AFTER INSERT OR UPDATE OR DELETE ON rsvp.reservations
    FOR EACH ROW EXECUTE PROCEDURE rsvp.reservations_trigger();

```

Here we use EXCLUDE constraint provided by postgresql to ensure that only one reservation can be made for a given resource at a given time.
ß
We use a trigger to notify the reservation_update channel when a reservation is created, updated or deleted. To make sure even we missed certain message from the channel, when the DB connection is re-established or some other reason, we use a queue to store the reservation changes. Thus we receive a notification from the channel, we can query the queue to get the reservation changes. Once we finish the processing, we can delete the record from the queue.

## Reference-level explanation

[reference-level-explanation]: #reference-level-explanation

This is the technical portion of the RFC. Explain the design in sufficient detail that:

* Its interaction with other features is clear.
* It is reasonably clear how the feature would be implemented.
* Corner cases are dissected by example.

The section should return to the examples given in the previous section, and explain more fully how the detailed proposal makes those examples work.

## Drawbacks

[drawbacks]: #drawbacks

Why should we *not* do this?

## Rationale and alternatives

[rationale-and-alternatives]: #rationale-and-alternatives

* Why is this design the best in the space of possible designs?
* What other designs have been considered and what is the rationale for not choosing them?
* What is the impact of not doing this?
* If this is a language proposal, could this be done in a library or macro instead? Does the proposed change make Rust code easier or harder to read, understand, and maintain?

## Prior art

[prior-art]: #prior-art

Discuss prior art, both the good and the bad, in relation to this proposal.
A few examples of what this can include are:

* For language, library, cargo, tools, and compiler proposals: Does this feature exist in other programming languages and what experience have their community had?
* For community proposals: Is this done by some other community and what were their experiences with it?
* For other teams: What lessons can we learn from what other communities have done here?
* Papers: Are there any published papers or great posts that discuss this? If you have some relevant papers to refer to, this can serve as a more detailed theoretical background.

This section is intended to encourage you as an author to think about the lessons from other languages, provide readers of your RFC with a fuller picture.
If there is no prior art, that is fine - your ideas are interesting to us whether they are brand new or if it is an adaptation from other languages.

Note that while precedent set by other languages is some motivation, it does not on its own motivate an RFC.
Please also take into consideration that rust sometimes intentionally diverges from common language features.

## Unresolved questions

[unresolved-questions]: #unresolved-questions

* What parts of the design do you expect to resolve through the RFC process before this gets merged?
* What parts of the design do you expect to resolve through the implementation of this feature before stabilization?
* What related issues do you consider out of scope for this RFC that could be addressed in the future independently of the solution that comes out of this RFC?

## Future possibilities

[future-possibilities]: #future-possibilities

Think about what the natural extension and evolution of your proposal would
be and how it would affect the language and project as a whole in a holistic
way. Try to use this section as a tool to more fully consider all possible
interactions with the project and language in your proposal.
Also consider how this all fits into the roadmap for the project
and of the relevant sub-team.

This is also a good place to "dump ideas", if they are out of scope for the
RFC you are writing but otherwise related.

If you have tried and cannot think of any future possibilities,
you may simply state that you cannot think of anything.

Note that having something written down in the future-possibilities section
is not a reason to accept the current or a future RFC; such notes should be
in the section on motivation or rationale in this or subsequent RFCs.
The section merely provides additional information.
