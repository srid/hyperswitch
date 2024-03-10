// Validate status 2xx 
pm.test("[POST]::/payouts/create - Status code is 2xx", function () {
   pm.response.to.be.success;
});

// Validate if response header has matching content-type
pm.test("[POST]::/payouts/create - Content-Type is application/json", function () {
   pm.expect(pm.response.headers.get("Content-Type")).to.include("application/json");
});

// Validate if response has JSON Body 
pm.test("[POST]::/payouts/create - Response has JSON Body", function () {
    pm.response.to.have.jsonBody();
});


// Set response object as internal variable
let jsonData = {};
try {jsonData = pm.response.json();}catch(e){}

// pm.collectionVariables - Set payment_id as variable for jsonData.payment_id
if (jsonData?.payment_id) {
   pm.collectionVariables.set("payment_id", jsonData.payment_id);
   console.log("- use {{payment_id}} as collection variable for value",jsonData.payment_id);
} else {
   console.log('INFO - Unable to assign variable {{payment_id}}, as jsonData.payment_id is undefined.');
};


// pm.collectionVariables - Set mandate_id as variable for jsonData.mandate_id
if (jsonData?.mandate_id) {
   pm.collectionVariables.set("mandate_id", jsonData.mandate_id);
   console.log("- use {{mandate_id}} as collection variable for value",jsonData.mandate_id);
} else {
   console.log('INFO - Unable to assign variable {{mandate_id}}, as jsonData.mandate_id is undefined.');
};

// pm.collectionVariables - Set payout_id as variable for jsonData.payout_id
if (jsonData?.payout_id) {
   pm.collectionVariables.set("payout_id", jsonData.payout_id);
   console.log("- use {{payout_id}} as collection variable for value",jsonData.payout_id);
} else {
   console.log('INFO - Unable to assign variable {{payout_id}}, as jsonData.payout_id is undefined.');
};

// Response body should have value "wise" for "connector"
if (jsonData?.connector) {
  pm.test(
    "[POST]::/payouts/create - Content check if value for 'connector' matches 'wise'",
    function () {
      pm.expect(jsonData.connector).to.eql("wise");
    },
  );
}

// pm.collectionVariables - Set client_secret as variable for jsonData.client_secret
if (jsonData?.client_secret) {
   pm.collectionVariables.set("client_secret", jsonData.client_secret);
   console.log("- use {{client_secret}} as collection variable for value",jsonData.client_secret);
} else {
   console.log('INFO - Unable to assign variable {{client_secret}}, as jsonData.client_secret is undefined.');
};